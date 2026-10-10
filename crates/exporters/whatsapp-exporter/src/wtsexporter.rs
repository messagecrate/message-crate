//! Locate and run the external `wtsexporter` CLI.

use crate::ios_backup::DecryptedWhatsapp;
use anyhow::{Context, Result, bail};
use message_staging::scratch_disk_full;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where a copy of `wtsexporter` put in the Tools Directory by hand comes
/// from: a `wtsexporter_<platform>` file of Message Crate's fork at release
/// `0.13.0-mc.2`, the first that records `full_key_id` and `reply_key_id`, the
/// ids a quoted reply is linked by, and `reaction_details`, each reaction
/// under the reactor's id, renamed to the name `resolve_wtsexporter` looks
/// for. Upstream records none of them.
const RELEASE_FILE_HINT: &str = "a wtsexporter_<platform> file from the messagecrate/WhatsApp-Chat-Exporter 0.13.0-mc.2 release, renamed to wtsexporter (wtsexporter.exe on Windows) and made executable (chmod +x on Linux and macOS)";

/// The sentence that sends a person to the user guide's section on a
/// `wtsexporter` that is missing or doesn't run, which says how to get one
/// by hand, including where the app has no download of its own. It is the
/// desktop app's `tool_downloads::troubleshooting` sentence for wtsexporter.
const TROUBLESHOOTING: &str =
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
    pub business: bool,
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

/// Where `wtsexporter` is: the app's own copy in the Tools Directory the
/// desktop app named with [`media::set_tools_dir`]. `Ok(None)` when there is
/// no Tools Directory or no file in it.
///
/// # Errors
///
/// Returns an error when the file is there but is not executable.
pub fn wtsexporter_path() -> Result<Option<PathBuf>> {
    find_wtsexporter(media::tools_dir().as_deref())
}

/// Locate `wtsexporter` (the Python WhatsApp export tool this crate shells
/// out to) in the Tools Directory.
///
/// # Errors
///
/// Returns an error when it is not there.
pub(crate) fn resolve_wtsexporter() -> Result<PathBuf> {
    wtsexporter_in(media::tools_dir().as_deref())
}

/// `wtsexporter` in `tools_dir`, and nowhere else: it is always the app's
/// own copy, so neither `PATH` nor an environment variable is read (#1053).
/// `Ok(None)` when there is no Tools Directory or no file in it.
///
/// On Unix the file must have an executable bit. It is not run here: a
/// `pipx` shim is slow to start. A shim is what is linked there on a
/// computer the app has no download for, such as Linux on ARM. So a link
/// to a `pipx` shim whose Python has gone is found, and fails only when it
/// is run, with the hint [`run_wtsexporter`] gives (`docs/adr/0019`).
///
/// # Errors
///
/// Returns an error when the file is there but is not executable.
fn find_wtsexporter(tools_dir: Option<&Path>) -> Result<Option<PathBuf>> {
    let Some(tools_dir) = tools_dir else {
        return Ok(None);
    };
    let candidate = tools_dir.join(wtsexporter_file_name());
    if !candidate.is_file() {
        return Ok(None);
    }
    if !is_executable(&candidate) {
        bail!(
            "{} is not executable. It must be {RELEASE_FILE_HINT}.",
            candidate.display()
        );
    }
    Ok(Some(candidate))
}

/// Whether `path` has an executable bit.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

/// Windows runs any `.exe`, so there is no bit to check.
#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

/// [`find_wtsexporter`], with a missing file as an error.
///
/// # Errors
///
/// Returns an error naming the Tools Directory when the file is not in it,
/// saying there is none, or saying the file is not executable.
fn wtsexporter_in(tools_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(found) = find_wtsexporter(tools_dir)? {
        return Ok(found);
    }
    let executable = wtsexporter_file_name();
    let Some(tools_dir) = tools_dir else {
        bail!(
            "Could not find {executable}: no Tools Directory is set. The desktop app keeps \
             {executable} in its Tools Directory."
        );
    };
    bail!(
        "Could not find {executable} in the Tools Directory, {}. Put {RELEASE_FILE_HINT} there. \
         {TROUBLESHOOTING}",
        tools_dir.display()
    );
}

/// Run wtsexporter in `args.work_dir`; write JSON to `json_out`.
/// Returns stderr+stdout for logging.
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
                    " (often a link to a pipx install whose Python interpreter has gone. {TROUBLESHOOTING})"
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
    if args.business {
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
mod tests {
    use super::{
        Platform, TROUBLESHOOTING, WtsexporterArgs, android_crypt_backup, extracts_ios_backup,
        input_search_root, names_a_full_disk, resolve_forwarded_paths, run_wtsexporter,
        scratch_write_error, wtsexporter_command, wtsexporter_file_name, wtsexporter_in,
    };
    use crate::ios_backup::DecryptedWhatsapp;
    use media::testutil::write_with_mode;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn android_args(input: &Path, key: Option<&str>) -> WtsexporterArgs {
        WtsexporterArgs {
            platform: Platform::Android,
            input: input.to_path_buf(),
            work_dir: input.to_path_buf(),
            key: key.map(str::to_string),
            backup: None,
            wa: None,
            media: None,
            db: None,
            business: false,
        }
    }

    /// wtsexporter looks for its default files (`msgstore.db`, `wa.db`, the
    /// key) in one directory. For a file input that is the directory holding the
    /// file, not the directory the app happens to run in.
    #[test]
    fn a_file_input_is_searched_in_the_directory_that_holds_it() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("msgstore.db");
        fs::write(&file, b"db").unwrap();
        assert_eq!(input_search_root(&file).unwrap(), dir.path());
        assert_eq!(input_search_root(dir.path()).unwrap(), dir.path());
    }

    #[test]
    fn prefers_msgstore_db_over_crypt() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("msgstore.db"), b"db").unwrap();
        fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
        assert_eq!(android_crypt_backup(dir.path()), None);
    }

    #[test]
    fn finds_crypt15_when_msgstore_missing() {
        let dir = tempdir().unwrap();
        let crypt = dir.path().join("msgstore.db.crypt15");
        fs::write(&crypt, b"crypt").unwrap();
        assert_eq!(
            android_crypt_backup(dir.path()).as_deref(),
            Some(crypt.as_path())
        );
    }

    #[test]
    fn prefers_crypt12_over_crypt15() {
        let dir = tempdir().unwrap();
        let crypt12 = dir.path().join("msgstore.db.crypt12");
        fs::write(&crypt12, b"c12").unwrap();
        fs::write(dir.path().join("msgstore.db.crypt15"), b"c15").unwrap();
        assert_eq!(
            android_crypt_backup(dir.path()).as_deref(),
            Some(crypt12.as_path())
        );
    }

    #[test]
    fn ignores_crypt15_directory() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("msgstore.db.crypt15")).unwrap();
        assert_eq!(android_crypt_backup(dir.path()), None);
    }

    #[test]
    fn drops_key_when_msgstore_db_is_present() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("msgstore.db"), b"db").unwrap();
        fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
        let paths = resolve_forwarded_paths(&android_args(dir.path(), Some("deadbeef"))).unwrap();
        assert!(paths.backup.is_none());
        assert!(paths.key.is_none());
    }

    #[test]
    fn forwards_crypt15_and_key_when_msgstore_missing() {
        let dir = tempdir().unwrap();
        let crypt = dir.path().join("msgstore.db.crypt15");
        fs::write(&crypt, b"crypt").unwrap();
        let paths = resolve_forwarded_paths(&android_args(dir.path(), Some("deadbeef"))).unwrap();
        assert_eq!(paths.backup.as_deref(), Some(crypt.as_path()));
        assert_eq!(paths.key.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn forwards_crypt15_without_key_when_key_omitted() {
        let dir = tempdir().unwrap();
        let crypt = dir.path().join("msgstore.db.crypt15");
        fs::write(&crypt, b"crypt").unwrap();
        let paths = resolve_forwarded_paths(&android_args(dir.path(), None)).unwrap();
        assert_eq!(paths.backup.as_deref(), Some(crypt.as_path()));
        assert!(paths.key.is_none());
    }

    /// The arguments of the command built for `args`, as strings.
    fn command_args(args: &WtsexporterArgs, out_dir: &Path, json_out: &Path) -> Vec<String> {
        let cmd = wtsexporter_command(Path::new("wtsexporter"), args, out_dir, json_out).unwrap();
        assert_eq!(cmd.get_current_dir(), Some(args.work_dir.as_path()));
        cmd.get_args()
            .map(|arg| arg.to_str().unwrap().to_string())
            .collect()
    }

    fn text(path: &Path) -> String {
        path.to_str().unwrap().to_string()
    }

    /// An Android backup with every option found: an encrypted database,
    /// a hex key (passed as a key file, never on the command line),
    /// contacts, media, and the business app. The whole command line is
    /// pinned, so a flag added in any form (`-c`, which moves the user's
    /// media into the scratch directory, above all) fails here.
    #[test]
    fn an_android_command_forwards_every_found_path_and_nothing_else() {
        let dir = tempdir().unwrap();
        let crypt = dir.path().join("msgstore.db.crypt15");
        fs::write(&crypt, b"crypt").unwrap();
        let wa = dir.path().join("wa.db");
        fs::write(&wa, b"wa").unwrap();
        let media = dir.path().join("WhatsApp");
        fs::create_dir(&media).unwrap();
        let work = tempdir().unwrap();
        let out = work.path().join("out");
        let json = out.join("result.json");
        let mut args = android_args(dir.path(), Some("deadbeef"));
        args.work_dir = work.path().to_path_buf();
        args.business = true;

        let key_file = work.path().join("decryption.key");
        assert_eq!(
            command_args(&args, &out, &json),
            [
                "-a".to_string(),
                "--no-html".to_string(),
                "--no-banner".to_string(),
                "-o".to_string(),
                text(&out),
                "-j".to_string(),
                text(&json),
                "-k".to_string(),
                text(&key_file),
                "-b".to_string(),
                text(&crypt),
                "-w".to_string(),
                text(&wa),
                "-m".to_string(),
                text(&media),
                "--business".to_string(),
            ]
        );
        assert_eq!(fs::read(&key_file).unwrap(), [0xde, 0xad, 0xbe, 0xef]);
    }

    /// A key given as a file path is passed to wtsexporter as that path,
    /// made absolute, rather than read as hex key material. A path is told
    /// from hex by a slash or by the `.key` ending.
    #[test]
    fn a_key_file_path_is_forwarded_as_a_path() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
        let work = tempdir().unwrap();
        let out = work.path().join("out");
        let json = out.join("result.json");
        let cwd = std::env::current_dir().unwrap();

        let absolute = dir.path().join("backup.key");
        for (key, expected) in [
            (text(&absolute), absolute.clone()),
            ("x.key".to_string(), cwd.join("x.key")),
            ("keys/key".to_string(), cwd.join("keys/key")),
        ] {
            let mut args = android_args(dir.path(), Some(&key));
            args.work_dir = work.path().to_path_buf();
            let command = command_args(&args, &out, &json);
            let k = command.iter().position(|a| a == "-k").expect("a -k flag");
            assert_eq!(command[k + 1], text(&expected), "{key}");
            assert!(
                !work.path().join("decryption.key").exists(),
                "{key} was read as hex"
            );
        }
    }

    /// A database file chosen as the input is passed with `-d`, whatever it
    /// is called.
    #[test]
    fn a_database_file_given_as_the_input_is_passed_as_the_database() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("phone-msgstore.db");
        fs::write(&db, b"db").unwrap();
        let mut args = android_args(&db, None);
        args.work_dir = dir.path().to_path_buf();
        let out = dir.path().join("out");
        let command = command_args(&args, &out, &out.join("result.json"));
        let d = command.iter().position(|a| a == "-d").expect("a -d flag");
        assert_eq!(command[d + 1], text(&db));
    }

    /// Only Android has crypt backup files, so an iOS directory that happens to
    /// hold one passes no `-b`.
    #[test]
    fn an_ios_directory_forwards_no_android_backup() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
        let args = WtsexporterArgs {
            platform: Platform::Ios,
            ..android_args(dir.path(), None)
        };
        let paths = resolve_forwarded_paths(&args).unwrap();
        assert!(paths.backup.is_none());
    }

    /// An encrypted iPhone backup is never passed to wtsexporter: the
    /// command names the decrypted database, contacts and media, and has
    /// no `-b`.
    #[test]
    fn a_decrypted_iphone_backup_is_read_from_its_files_with_no_backup_flag() {
        let backup = tempdir().unwrap();
        let work = tempdir().unwrap();
        let domain = work
            .path()
            .join("AppDomainGroup-group.net.whatsapp.WhatsApp.shared");
        fs::create_dir(&domain).unwrap();
        let db = domain.join("ChatStorage.sqlite");
        fs::write(&db, b"db").unwrap();
        let contacts = domain.join("ContactsV2.sqlite");
        fs::write(&contacts, b"contacts").unwrap();
        let out = work.path().join("out");
        let json = out.join("result.json");
        let mut args = WtsexporterArgs {
            platform: Platform::Ios,
            backup: Some(backup.path().to_path_buf()),
            work_dir: work.path().to_path_buf(),
            ..android_args(backup.path(), None)
        };
        args.read_decrypted(DecryptedWhatsapp {
            domain_dir: domain.clone(),
            database: db.clone(),
        });

        assert_eq!(
            command_args(&args, &out, &json),
            [
                "-i".to_string(),
                "--no-html".to_string(),
                "--no-banner".to_string(),
                "-o".to_string(),
                text(&out),
                "-j".to_string(),
                text(&json),
                "-d".to_string(),
                text(&db),
                "-w".to_string(),
                text(&contacts),
                "-m".to_string(),
                text(&domain),
            ]
        );
    }

    /// An iPhone backup is passed with `-b` alone. WhatsApp's own files at
    /// the backup directory's root are left over from an earlier extract there
    /// and are not passed: with the shared directory but no `ChatStorage.sqlite`
    /// at the root, `-m` and `-w` without `-d` make wtsexporter stop with
    /// "The message database does not exist".
    #[test]
    fn an_iphone_backup_is_passed_alone_without_the_files_at_its_root() {
        let backup = tempdir().unwrap();
        let shared = backup
            .path()
            .join("AppDomainGroup-group.net.whatsapp.WhatsApp.shared");
        fs::create_dir(&shared).unwrap();
        fs::write(shared.join("ContactsV2.sqlite"), b"contacts").unwrap();
        let work = tempdir().unwrap();
        let out = work.path().join("out");
        let json = out.join("result.json");
        let args = WtsexporterArgs {
            platform: Platform::Ios,
            backup: Some(backup.path().to_path_buf()),
            work_dir: work.path().to_path_buf(),
            ..android_args(backup.path(), None)
        };

        assert_eq!(
            command_args(&args, &out, &json),
            [
                "-i".to_string(),
                "--no-html".to_string(),
                "--no-banner".to_string(),
                "-o".to_string(),
                text(&out),
                "-j".to_string(),
                text(&json),
                "-b".to_string(),
                text(backup.path()),
            ]
        );
    }

    /// An iOS backup forwards its database, contacts and media found under
    /// the input directory; with no backup file there is no `-b`, so the key
    /// is dropped too.
    #[test]
    fn an_ios_command_forwards_the_found_database_contacts_and_media() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("ChatStorage.sqlite");
        fs::write(&db, b"db").unwrap();
        let shared = dir
            .path()
            .join("AppDomainGroup-group.net.whatsapp.WhatsApp.shared");
        fs::create_dir(&shared).unwrap();
        let contacts = shared.join("ContactsV2.sqlite");
        fs::write(&contacts, b"contacts").unwrap();
        let out = dir.path().join("out");
        let json = out.join("result.json");
        let args = WtsexporterArgs {
            platform: Platform::Ios,
            key: Some("deadbeef".to_string()),
            ..android_args(dir.path(), None)
        };

        assert_eq!(
            command_args(&args, &out, &json),
            [
                "-i".to_string(),
                "--no-html".to_string(),
                "--no-banner".to_string(),
                "-o".to_string(),
                text(&out),
                "-j".to_string(),
                text(&json),
                "-d".to_string(),
                text(&db),
                "-w".to_string(),
                text(&contacts),
                "-m".to_string(),
                text(&shared),
            ]
        );
    }

    /// wtsexporter extracts an iPhone backup's WhatsApp files into the work
    /// directory only when it gets the backup and no media directory that
    /// exists, so only then is there an extract to measure. Android never
    /// extracts one.
    #[test]
    fn only_an_iphone_backup_without_a_media_directory_is_extracted() {
        let dir = tempdir().unwrap();
        let backup = dir.path().join("backup");
        let media = dir.path().join("WhatsApp");
        fs::create_dir_all(&backup).unwrap();
        let ios = |media: Option<&Path>| WtsexporterArgs {
            platform: Platform::Ios,
            backup: Some(backup.clone()),
            media: media.map(Path::to_path_buf),
            ..android_args(&backup, None)
        };

        assert!(extracts_ios_backup(&ios(None)).unwrap());
        assert!(
            extracts_ios_backup(&ios(Some(&media))).unwrap(),
            "a media directory that does not exist is no reason to skip"
        );
        fs::create_dir_all(&media).unwrap();
        assert!(!extracts_ios_backup(&ios(Some(&media))).unwrap());
        let no_backup = WtsexporterArgs {
            backup: None,
            ..ios(None)
        };
        assert!(!extracts_ios_backup(&no_backup).unwrap());
        assert!(!extracts_ios_backup(&android_args(&backup, None)).unwrap());
    }

    /// Held while a test writes a stand-in wtsexporter and runs it. A file
    /// still open for writing cannot be run ("Text file busy"), and a test
    /// that starts a process on another thread holds a copy of every open
    /// file until that process starts; one at a time, no stand-in is being
    /// written while another starts.
    #[cfg(unix)]
    static STAND_IN: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A stand-in for wtsexporter in `dir`: it writes part of a decrypted
    /// `msgstore.db` into its working directory, as the real one does before
    /// its disk fills, prints `error` and exits 1. The caller holds
    /// [`STAND_IN`] until it has run it.
    #[cfg(unix)]
    fn failing_wtsexporter(dir: &Path, error: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.join("wtsexporter");
        fs::write(
            &bin,
            format!("#!/bin/sh\nprintf partial > msgstore.db\necho '{error}' >&2\nexit 1\n"),
        )
        .unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    /// The Android arguments for a run whose work directory is `work`.
    #[cfg(unix)]
    fn android_run_args(input: &Path, work: &Path) -> WtsexporterArgs {
        WtsexporterArgs {
            work_dir: work.to_path_buf(),
            ..android_args(input, Some("deadbeef"))
        }
    }

    /// wtsexporter decrypts an Android backup's `msgstore.db` into the work
    /// directory, under the Scratch Directory. When that disk fills, the run
    /// says so in the sentence every free-space check gives, naming the
    /// Scratch Directory's disk, in place of wtsexporter's raw error (#1820).
    #[cfg(unix)]
    #[test]
    fn a_full_scratch_disk_is_reported_as_the_free_space_sentence() {
        let _one_at_a_time = STAND_IN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempdir().unwrap();
        let input = dir.path().join("backup");
        let work = dir.path().join("work");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&work).unwrap();
        let bin = failing_wtsexporter(dir.path(), "OSError: [Errno 28] No space left on device");

        let err = run_wtsexporter(
            &bin,
            &android_run_args(&input, &work),
            &work.join("result.json"),
        )
        .unwrap_err()
        .to_string();

        assert!(
            err.starts_with("Not enough space on the disk that holds the Scratch Directory"),
            "{err}"
        );
        assert!(!err.contains("wtsexporter failed"), "{err}");
        assert!(!err.contains("Errno 28"), "{err}");
    }

    /// Any other wtsexporter failure is reported as wtsexporter gave it.
    #[cfg(unix)]
    #[test]
    fn any_other_wtsexporter_failure_is_reported_as_it_is() {
        let _one_at_a_time = STAND_IN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempdir().unwrap();
        let input = dir.path().join("backup");
        let work = dir.path().join("work");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&work).unwrap();
        let bin = failing_wtsexporter(dir.path(), "ValueError: The key is incorrect");

        let err = run_wtsexporter(
            &bin,
            &android_run_args(&input, &work),
            &work.join("result.json"),
        )
        .unwrap_err()
        .to_string();

        assert!(err.starts_with("wtsexporter failed ("), "{err}");
        assert!(err.contains("ValueError: The key is incorrect"), "{err}");
    }

    /// The partial `msgstore.db` a full disk leaves is in the run's work
    /// directory, so it goes when the run lets go of that directory, as it
    /// does when wtsexporter's error ends the run.
    #[cfg(unix)]
    #[test]
    fn the_partial_database_goes_with_the_work_directory() {
        let _one_at_a_time = STAND_IN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempdir().unwrap();
        let input = dir.path().join("backup");
        fs::create_dir_all(&input).unwrap();
        let work = message_crate_core::ScratchDir::create(
            &dir.path()
                .join("scratch")
                .join(message_crate_core::WHATSAPP_DIRECTORY),
        )
        .unwrap();
        let bin = failing_wtsexporter(dir.path(), "OSError: [Errno 28] No space left on device");

        run_wtsexporter(
            &bin,
            &android_run_args(&input, work.path()),
            &work.path().join("result.json"),
        )
        .unwrap_err();
        let partial = work.path().join("msgstore.db");
        assert!(
            partial.is_file(),
            "wtsexporter wrote into the work directory"
        );

        drop(work);
        assert!(!partial.exists(), "the partial database is deleted");
    }

    /// Python's error codes for a full disk name it in any system language,
    /// so a German Windows' `ERROR_DISK_FULL` is still the free-space error.
    #[test]
    fn a_full_disk_is_found_by_its_error_code_in_any_language() {
        assert!(names_a_full_disk(
            "OSError: [WinError 112] Auf dem Datenträger ist nicht genug Speicherplatz vorhanden"
        ));
        assert!(names_a_full_disk(
            "OSError: [Errno 28] Espace insuffisant sur le périphérique"
        ));
        assert!(!names_a_full_disk("ValueError: The key is incorrect"));
    }

    /// A Scratch Directory disk already full before wtsexporter starts gives
    /// the same free-space error as one that fills while it runs; any other
    /// write error keeps its own words.
    #[test]
    fn a_full_disk_before_wtsexporter_starts_is_the_free_space_error() {
        let full = scratch_write_error(
            std::io::Error::from(std::io::ErrorKind::StorageFull),
            "create work/decryption.key".to_string(),
        )
        .to_string();
        assert!(
            full.starts_with("Not enough space on the disk that holds the Scratch Directory"),
            "{full}"
        );

        let denied = scratch_write_error(
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            "create work/decryption.key".to_string(),
        );
        assert_eq!(denied.to_string(), "create work/decryption.key");
    }

    /// wtsexporter is always the app's own copy in the Tools Directory.
    #[test]
    fn wtsexporter_is_found_in_the_tools_directory() {
        let tools = tempfile::tempdir().unwrap();
        let program = tools.path().join(wtsexporter_file_name());
        write_with_mode(&program, 0o755);

        assert_eq!(wtsexporter_in(Some(tools.path())).unwrap(), program);
    }

    /// A release file put in the Tools Directory without `chmod +x` cannot
    /// start, so it is not reported as found, and the error says how to fix it.
    #[cfg(unix)]
    #[test]
    fn a_wtsexporter_that_is_not_executable_is_refused() {
        let tools = tempfile::tempdir().unwrap();
        let program = tools.path().join(wtsexporter_file_name());
        write_with_mode(&program, 0o644);

        let err = wtsexporter_in(Some(tools.path())).expect_err("not executable");
        let message = err.to_string();
        assert!(message.contains("is not executable"), "{message}");
        assert!(message.contains("chmod +x"), "{message}");
        assert!(
            super::find_wtsexporter(Some(tools.path())).is_err(),
            "the status lookup refuses it too"
        );
    }

    /// A missing copy is reported as missing, naming the Tools Directory,
    /// and a copy outside it is not found (#1053).
    #[test]
    fn wtsexporter_is_looked_for_nowhere_but_the_tools_directory() {
        let tools = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        write_with_mode(&elsewhere.path().join(wtsexporter_file_name()), 0o755);

        let err = wtsexporter_in(Some(tools.path())).expect_err("not in the Tools Directory");
        let no_dir = wtsexporter_in(None).expect_err("no Tools Directory");

        let message = err.to_string();
        assert!(message.contains("Tools Directory"), "{message}");
        // A pipx install linked there is replaced by the app's own download,
        // so the error sends the person to the guide, not to pipx.
        assert!(message.ends_with(TROUBLESHOOTING), "{message}");
        assert!(!message.contains("pipx"), "{message}");
        assert!(
            message.contains(&tools.path().display().to_string()),
            "{message}"
        );
        assert!(
            no_dir.to_string().contains("no Tools Directory"),
            "{no_dir}"
        );
    }
}
