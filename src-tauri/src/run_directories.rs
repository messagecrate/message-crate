//! The Staging Directory and the run directories this app made under it.
//!
//! The desktop process owns both. The window never names a root:
//! `set_staging_root` stores the Staging Directory here, `create_run_dir`
//! makes a run's directory under it, and every other command that touches a
//! run directory takes only the directory.
//!
//! A directory is acted on when this app made it, which this record says, and
//! when it still holds the `.message-crate-export` sentinel. Where the
//! Staging Directory points now does not matter. A run keeps its directory
//! when the setting changes, and the new setting applies to the directories
//! made after it (issue #1154).
//!
//! The record lives in one JSON file in the app-data directory
//! ([`RECORD_FILE`]), so it survives a restart, as a paused Import Run does.
//! That file is the only copy: every read takes a shared lock on
//! [`LOCK_FILE`], and every change takes it alone and reads the file again
//! before it writes. Two app processes at once (two launches, or a dev build
//! beside an installed one) then never write over each other's directories.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use ir_format::{EXPORT_SENTINEL, mark_export_directory};

use crate::app_directories::{RunLog, import_run_log};
use crate::commands::paths::{require_absolute, resolve_openable_path, resolve_staging_root};
use crate::run_logs::account_of;

/// File in the app-data directory that holds the Staging Directory setting
/// and the run directories this app made.
pub const RECORD_FILE: &str = "staging.json";

/// File beside [`RECORD_FILE`] that every process locks to read or change it.
const LOCK_FILE: &str = "staging.json.lock";

/// What [`RECORD_FILE`] holds.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Record {
    /// The Staging Directory from Settings. `None` means the default,
    /// `{home}/message-crate/staging`.
    root: Option<PathBuf>,
    /// Every run directory this app made and has not deleted.
    directories: Vec<MadeDirectory>,
}

/// One run directory this app made.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct MadeDirectory {
    /// The directory, in the canonical form it was given when it was made.
    path: PathBuf,
    /// The device the directory was made on, where the operating system reports
    /// one. A directory whose parent is now on another device, such as the
    /// empty mount point of an unplugged drive, is not counted as deleted.
    device: Option<u64>,
}

/// The Staging Directory as Settings show it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagingRoot {
    /// The directory new run directories are made in.
    pub root: String,
    /// `{home}/message-crate/staging`: the directory used when Settings name
    /// none.
    pub default_root: String,
}

/// A run directory this app made, and the Import Run its log says it holds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryOfRun {
    /// The directory, as it was recorded when it was made.
    pub run_dir: String,
    /// The Import Run's id on the Message Crate it imported into.
    pub import_run_id: i64,
}

/// An account on one Message Crate: the signed-in account, which a run's log
/// must name for the app to act on the run's directory.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedIn {
    /// The account's id on that Message Crate.
    pub account_id: i64,
    /// The id of the Message Crate, as `GET /v1/server` answers it.
    pub message_crate_id: String,
}

impl SignedIn {
    /// Whether the run log's account line `account` names this account on
    /// this Message Crate.
    fn ran(&self, account: &message_crate_core::RunLogAccount) -> bool {
        account.account_id == self.account_id && account.message_crate_id == self.message_crate_id
    }
}

/// The directory of a closed Import Run, and the run as its log names it.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedRun {
    /// The run's directory, as [`RunDirectories::runs_of`] gave it.
    pub run_dir: String,
    /// The Import Run's id on the Message Crate it imported into.
    pub import_run_id: i64,
    /// The account that ran it, on that Message Crate.
    #[serde(flatten)]
    pub signed_in: SignedIn,
    /// How the server reports the run ended.
    pub status: ClosedStatus,
}

/// How a closed Import Run ended, in the words the server's `status` uses.
/// `running` is not one of them, so a running run's directory cannot be
/// handed to [`RunDirectories::delete_closed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClosedStatus {
    Completed,
    CompletedWithIssues,
    Failed,
    Canceled,
}

impl ClosedStatus {
    /// The status as the server writes it.
    fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::CompletedWithIssues => "completed_with_issues",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }
}

/// The Staging Directory and the run directories made under it, kept in
/// [`RECORD_FILE`].
#[derive(Debug)]
pub struct RunDirectories {
    /// Where the record is saved.
    file: PathBuf,
    /// The default Staging Directory, `staging` in the Message Crate
    /// Directory.
    default_root: PathBuf,
    /// The Logs Directory, where each run's log outlives its directory. A
    /// name whose log is there is not given to a new run, so two runs never
    /// share a log.
    logs: PathBuf,
    /// Held for the whole of each delete, so two deletes of one directory in
    /// this process, such as a discard and the clean-up when Import opens,
    /// run one after the other. The second then finds the directory gone,
    /// instead of failing on a file the first removed under it.
    deleting: Mutex<()>,
}

impl RunDirectories {
    /// The record kept in `file`, read from it on every use, with
    /// `default_root` as the Staging Directory when Settings name none, for
    /// runs whose logs go to `logs`.
    pub fn at(file: PathBuf, default_root: PathBuf, logs: PathBuf) -> Self {
        Self {
            file,
            default_root,
            logs,
            deleting: Mutex::new(()),
        }
    }

    /// The record as the file holds it now.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be locked or read.
    fn read(&self) -> Result<Record, String> {
        let _lock = self.lock_file(false)?;
        self.read_locked()
    }

    /// Change the record with `change`, holding the lock from the read to the
    /// write, so no other process's change is written over.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be locked, read or written, or
    /// `change` refuses.
    fn change<T>(
        &self,
        change: impl FnOnce(&mut Record) -> Result<T, String>,
    ) -> Result<T, String> {
        let _lock = self.lock_file(true)?;
        let mut record = self.read_locked()?;
        let answer = change(&mut record)?;
        self.write_locked(&record)?;
        Ok(answer)
    }

    /// [`LOCK_FILE`], locked alone when `exclusive`, shared otherwise. The
    /// lock is released when the file is dropped.
    fn lock_file(&self, exclusive: bool) -> Result<File, String> {
        let dir = self.file.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("Could not make {}: {error}", dir.display()))?;
        let path = dir.join(LOCK_FILE);
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
        if exclusive {
            file.lock()
        } else {
            file.lock_shared()
        }
        .map_err(|error| format!("Could not lock {}: {error}", path.display()))?;
        Ok(file)
    }

    /// The record in [`RECORD_FILE`], or an empty one when there is no file
    /// yet. A file that cannot be read or parsed is an error, never an empty
    /// record, so the next change cannot write over the directories it lists.
    fn read_locked(&self) -> Result<Record, String> {
        match std::fs::read(&self.file) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                format!(
                    "{} is damaged ({error}). Move it aside to start a new record of run directories",
                    self.file.display()
                )
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Record::default()),
            Err(error) => Err(format!("Could not read {}: {error}", self.file.display())),
        }
    }

    /// The Staging Directory new run directories are made in.
    ///
    /// # Errors
    ///
    /// Returns an error when the record cannot be read.
    pub fn root(&self) -> Result<PathBuf, String> {
        Ok(self
            .read()?
            .root
            .unwrap_or_else(|| self.default_root.clone()))
    }

    /// The Staging Directory and its default, for Settings.
    ///
    /// # Errors
    ///
    /// Returns an error when the record cannot be read.
    pub fn describe(&self) -> Result<StagingRoot, String> {
        Ok(StagingRoot {
            root: self.root()?.display().to_string(),
            default_root: self.default_root.display().to_string(),
        })
    }

    /// Store the Staging Directory from Settings. An empty value, or the
    /// default itself, goes back to the default. Directories made under the
    /// earlier setting keep working.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory is relative or the filesystem root,
    /// or the record cannot be saved.
    pub fn set_root(&self, root: &str) -> Result<(), String> {
        let trimmed = root.trim();
        let root = if trimmed.is_empty() {
            None
        } else {
            resolve_staging_root(trimmed)?;
            let path = PathBuf::from(trimmed);
            if path == self.default_root {
                None
            } else {
                Some(path)
            }
        };
        self.change(|record| {
            record.root = root;
            Ok(())
        })
    }

    /// Make a new run directory, `staging-<label>-<timestamp>`, under the
    /// Staging Directory, write the export sentinel into it, and record it.
    /// Returns the directory in its canonical form.
    ///
    /// `label` names the Import source the directory is for, such as
    /// `imessage-ios`.
    ///
    /// # Errors
    ///
    /// Returns an error when `label` is not lowercase letters, digits and
    /// dashes, the Staging Directory is unusable, or the directory, its
    /// sentinel or the record cannot be written.
    pub fn create(&self, label: &str, timestamp: &str) -> Result<PathBuf, String> {
        let slug = directory_slug(label)?;
        let root = resolve_staging_root(&self.root()?.display().to_string())?;
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("Could not make {}: {error}", root.display()))?;
        let base = format!("staging-{slug}-{timestamp}");
        let mut directory = root.join(&base);
        let mut n = 1;
        loop {
            // An earlier run of that name, perhaps in another Staging
            // Directory or before the clock went back, left its log.
            if import_run_log(&self.logs, &directory).exists() {
                n += 1;
                directory = root.join(format!("{base}-{n}"));
                continue;
            }
            // Owner-only whatever the umask: the run's staged conversations
            // go in it (#2969). The Staging Directory is the person's and
            // keeps its mode.
            match file_io::create_dir_owner_only(&directory) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    n += 1;
                    directory = root.join(format!("{base}-{n}"));
                }
                Err(error) => {
                    return Err(format!("Could not make {}: {error}", directory.display()));
                }
            }
        }
        mark_export_directory(&directory)
            .map_err(|error| format!("Could not mark {}: {error:#}", directory.display()))?;
        let directory = directory
            .canonicalize()
            .map_err(|error| format!("Could not resolve {}: {error}", directory.display()))?;
        self.change(|record| {
            record.directories.push(MadeDirectory {
                path: directory.clone(),
                device: std::fs::metadata(&directory)
                    .ok()
                    .as_ref()
                    .and_then(device_of),
            });
            Ok(())
        })?;
        Ok(directory)
    }

    /// The run directory `dir`, when this app made it and it still holds the
    /// export sentinel. Where the Staging Directory points now plays no part.
    ///
    /// # Errors
    ///
    /// Returns an error when `dir` is empty or relative, is not on disk, was
    /// not made by [`RunDirectories::create`], or has no sentinel.
    pub fn directory(&self, dir: &str) -> Result<PathBuf, String> {
        let path = require_absolute(dir)?;
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("Could not find the run directory {dir}: {error}"))?;
        if !self
            .read()?
            .directories
            .iter()
            .any(|made| made.path == canonical)
        {
            return Err(format!(
                "{} is not a run directory Message Crate made",
                canonical.display()
            ));
        }
        if !canonical.join(EXPORT_SENTINEL).is_file() {
            return Err(format!(
                "{} does not look like an export directory (missing {EXPORT_SENTINEL})",
                canonical.display()
            ));
        }
        Ok(canonical)
    }

    /// Delete the run directory `dir` and forget it. A directory that is no
    /// longer on disk counts as deleted, but only when the directory it was in is
    /// still there: an unplugged drive, or a directory the app may not read, is
    /// an error, and the directory stays recorded.
    ///
    /// The record's lock is held only to check the directory and to forget it,
    /// not while a directory of several gigabytes is removed.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory fails [`RunDirectories::directory`]'s
    /// checks, cannot be removed, or the record cannot be saved.
    pub fn delete(&self, dir: &str) -> Result<(), String> {
        let _deleting = self.one_delete_at_a_time();
        self.delete_now(dir)
    }

    /// The lock that runs this process's deletes one at a time. A delete that
    /// panicked left nothing the next one relies on, so a poisoned lock is
    /// taken as it is.
    fn one_delete_at_a_time(&self) -> MutexGuard<'_, ()> {
        self.deleting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The work of [`RunDirectories::delete`], with the delete lock held.
    fn delete_now(&self, dir: &str) -> Result<(), String> {
        let path = require_absolute(dir)?;
        let made = self
            .read()?
            .directories
            .into_iter()
            .find(|made| made.path == path);
        if is_gone(&path, made.as_ref().and_then(|made| made.device)) {
            return self.forget(&path);
        }
        // A delete that stopped after the sentinel went, as when another
        // program held the emptied directory open, left it empty. It is finished
        // rather than refused for the missing sentinel.
        if made.is_some() && is_empty_dir(&path) {
            std::fs::remove_dir(&path)
                .map_err(|error| format!("Could not delete {}: {error}", path.display()))?;
            return self.forget(&path);
        }
        let directory = self.directory(dir)?;
        remove_sentinel_last(&directory)
            .map_err(|error| format!("Could not delete {}: {error}", directory.display()))?;
        self.forget(&directory)
    }

    /// The run directories this app made whose Import Run was run by
    /// `signed_in`, each with that run's id. A directory is tied to its run by
    /// the first line of the run's log ([`message_crate_core::RunLogAccount`]),
    /// which the window writes once the server has created the run. A
    /// directory whose log names no run, or names another account or another
    /// Message Crate, is left out.
    ///
    /// # Errors
    ///
    /// Returns an error when the record cannot be read.
    pub fn runs_of(&self, signed_in: &SignedIn) -> Result<Vec<DirectoryOfRun>, String> {
        Ok(self.runs_in(&made_paths(self.read()?), signed_in))
    }

    /// The directories among `made` whose log ties them to a run of
    /// `signed_in`, each with that run's id.
    fn runs_in(&self, made: &[PathBuf], signed_in: &SignedIn) -> Vec<DirectoryOfRun> {
        made.iter()
            .filter_map(|path| {
                let account = account_of(&import_run_log(&self.logs, path))?;
                signed_in.ran(&account).then(|| DirectoryOfRun {
                    run_dir: path.display().to_string(),
                    import_run_id: account.import_run_id,
                })
            })
            .collect()
    }

    /// The disk space the run directories of `signed_in`'s Import Runs take
    /// on this computer, in bytes: the directories [`RunDirectories::runs_of`]
    /// lists, measured now. A run directory made inside another, as when the
    /// Staging Directory was set inside a run's directory, counts once, as
    /// its own run's.
    ///
    /// # Errors
    ///
    /// Returns an error when the record cannot be read.
    pub fn bytes_of(&self, signed_in: &SignedIn) -> Result<u64, String> {
        // One read, so the runs measured and the runs skipped inside them
        // come from the same record.
        let made = made_paths(self.read()?);
        Ok(self
            .runs_in(&made, signed_in)
            .iter()
            .map(|run| {
                let run = Path::new(&run.run_dir);
                directory_bytes(run, &|path| path != run && made.iter().any(|m| m == path))
            })
            .sum())
    }

    /// The disk space the whole Staging Directory takes on this computer, in
    /// bytes, measured now: everything in the directory Settings name, for
    /// every account and every Message Crate this computer imported into,
    /// and every run directory this app made under an earlier setting and
    /// still keeps. Each byte counts once, wherever one of these directories
    /// sits inside another.
    ///
    /// # Errors
    ///
    /// Returns an error when the record cannot be read.
    pub fn bytes_of_all(&self) -> Result<u64, String> {
        let record = self.read()?;
        let root = record
            .root
            .clone()
            .unwrap_or_else(|| self.default_root.clone());
        let mut tops = made_paths(record);
        // A Staging Directory not made yet holds nothing.
        if let Ok(root) = root.canonicalize() {
            tops.push(root);
        }
        let inside_another =
            |path: &PathBuf| tops.iter().any(|top| top != path && path.starts_with(top));
        let mut counted: Vec<&PathBuf> = tops.iter().filter(|top| !inside_another(top)).collect();
        counted.sort();
        counted.dedup();
        Ok(counted
            .into_iter()
            .map(|top| directory_bytes(top, &|_| false))
            .sum())
    }

    /// Delete the directory of the closed Import Run `closed` names, and say
    /// so in the run's log, naming the run and the directory. A closed run's
    /// files are never read again. Returns whether a directory was removed:
    /// one already gone is only forgotten, and the log says nothing.
    ///
    /// # Errors
    ///
    /// Returns an error when the run's log does not name that run, account
    /// and Message Crate, so the directory is not tied to that run, or when
    /// the delete fails as [`RunDirectories::delete`] does.
    pub fn delete_closed(&self, closed: &ClosedRun) -> Result<bool, String> {
        let path = require_absolute(&closed.run_dir)?;
        let import_run_id = closed.import_run_id;
        // Import Run ids are counted per Message Crate, so the run's id alone
        // could name another Message Crate's run, still running there.
        let tied = account_of(&import_run_log(&self.logs, &path)).is_some_and(|account| {
            account.import_run_id == import_run_id && closed.signed_in.ran(&account)
        });
        if !tied {
            return Err(format!(
                "{} is not the directory of Import Run {import_run_id}",
                path.display()
            ));
        }
        let on_disk = {
            let _deleting = self.one_delete_at_a_time();
            let on_disk = std::fs::symlink_metadata(&path).is_ok();
            self.delete_now(&closed.run_dir)?;
            on_disk
        };
        let status = closed.status;
        if on_disk {
            RunLog::open(&self.logs, &path).line(&format!(
                "Deleted the directory of Import Run {import_run_id}, {}, because the server reports the run as {}",
                path.display(),
                status.as_str()
            ));
        }
        Ok(on_disk)
    }

    /// Drop `directory` from the record.
    fn forget(&self, directory: &Path) -> Result<(), String> {
        self.change(|record| {
            record.directories.retain(|made| made.path != directory);
            Ok(())
        })
    }

    /// Resolve `path` for opening: a run directory this app made, or a file
    /// or directory inside one, such as its Upload log.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is empty or relative, or is in no
    /// run directory this app made.
    pub fn openable(&self, path: &str) -> Result<PathBuf, String> {
        require_absolute(path)?;
        self.read()?
            .directories
            .iter()
            .find_map(|made| resolve_openable_path(path, &made.path.display().to_string()).ok())
            .ok_or_else(|| {
                "Path is not in a directory Message Crate made in the Staging Directory".to_string()
            })
    }

    /// Write `record` to [`RECORD_FILE`] through a temporary file of this
    /// process's own, so a crash mid-write leaves the previous record whole.
    fn write_locked(&self, record: &Record) -> Result<(), String> {
        let body = serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?;
        let tmp = self
            .file
            .with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, body)
            .and_then(|()| std::fs::rename(&tmp, &self.file))
            .map_err(|error| format!("Could not save {}: {error}", self.file.display()))
    }
}

/// Whether `path` is known to be gone: not found, in a directory that is still
/// there on `device`, the device it was made on. A path that cannot be read
/// for any other reason is not gone, and neither is one whose parent is now
/// on another device, as an unplugged drive's empty mount point is.
fn is_gone(path: &Path, device: Option<u64>) -> bool {
    let not_found = matches!(
        std::fs::symlink_metadata(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    );
    not_found
        && path
            .parent()
            .and_then(|parent| std::fs::metadata(parent).ok())
            .is_some_and(|parent| {
                parent.is_dir() && device.is_none_or(|device| device_of(&parent) == Some(device))
            })
}

/// The device a file or directory is on, where the operating system reports one.
#[cfg(unix)]
fn device_of(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.dev())
}

/// The device a file or directory is on, where the operating system reports one.
#[cfg(not(unix))]
fn device_of(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

/// The run directories `record` lists.
fn made_paths(record: Record) -> Vec<PathBuf> {
    record
        .directories
        .into_iter()
        .map(|made| made.path)
        .collect()
}

/// The bytes the files under `directory` hold, `directory` included, leaving
/// out every directory under it that `skip` names. A symbolic link counts as
/// itself and is never followed, so a link out of a run directory does not
/// count what it points at. An entry that cannot be read counts as nothing:
/// the total is what could be measured.
fn directory_bytes(directory: &Path, skip: &dyn Fn(&Path) -> bool) -> u64 {
    let mut total = 0;
    let mut pending = vec![directory.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            if skip(&path) {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(&path) {
                pending.extend(entries.flatten().map(|entry| entry.path()));
            }
        } else {
            total += metadata.len();
        }
    }
    total
}

/// Whether `path` is a directory with nothing in it.
fn is_empty_dir(path: &Path) -> bool {
    std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

/// Remove `directory`, its export sentinel last. A removal that fails part-way
/// leaves the sentinel, so the directory is still one the app may delete, and a
/// later delete can finish it.
fn remove_sentinel_last(directory: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_name() == EXPORT_SENTINEL {
            continue;
        }
        // A link to a directory, or a Windows junction, is removed as a link and
        // never followed: `remove_dir_all` does that, where `remove_file`
        // fails on a directory link on Windows.
        let kind = entry.file_type()?;
        if kind.is_dir() || kind.is_symlink() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    std::fs::remove_file(directory.join(EXPORT_SENTINEL))?;
    std::fs::remove_dir(directory)
}

/// The short name a run directory carries for `label`: an Import source id
/// shortened the way the directory names have always read, or `label` itself.
fn directory_slug(label: &str) -> Result<&str, String> {
    let valid = !label.is_empty()
        && label.len() <= 40
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid {
        return Err(format!("{label:?} cannot name a run directory"));
    }
    Ok(match label {
        "imessage-ios" => "iphone-ios",
        "imessage-macos" => "macos",
        "imessage-jailbreak" => "iphone-jailbreak",
        other => other,
    })
}

/// The local date and time as `YYMMDD-HHMMSS`, for a run directory's name.
pub fn timestamp_now() -> String {
    chrono::Local::now().format("%y%m%d-%H%M%S").to_string()
}

/// A record of run directories in its own temporary app-data and home
/// directories, with the Message Crate Directory in that home, for tests here
/// and in the staging commands.
#[cfg(test)]
pub(crate) struct Scratch {
    pub directories: RunDirectories,
    pub app_data: tempfile::TempDir,
    pub home: tempfile::TempDir,
}

#[cfg(test)]
impl Scratch {
    pub fn new() -> Self {
        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let directories = Self::open(&app_data, &home);
        Self {
            directories,
            app_data,
            home,
        }
    }

    /// The record kept in `app_data`, with `home` as the home directory.
    fn open(app_data: &tempfile::TempDir, home: &tempfile::TempDir) -> RunDirectories {
        let directory = Self::directory_in(home);
        RunDirectories::at(
            app_data.path().join(RECORD_FILE),
            directory.staging(),
            directory.logs(),
        )
    }

    /// The Message Crate Directory in `home`.
    fn directory_in(home: &tempfile::TempDir) -> crate::app_directories::MessageCrateDirectory {
        crate::app_directories::MessageCrateDirectory::in_home(home.path(), false)
    }

    /// The Logs Directory the runs' logs go to.
    pub fn logs(&self) -> PathBuf {
        Self::directory_in(&self.home).logs()
    }

    /// The same record, as another process or a restarted app opens it.
    pub fn reopen(&self) -> RunDirectories {
        Self::open(&self.app_data, &self.home)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const NOW: &str = "261002-101500";

    #[test]
    fn a_run_keeps_its_directory_when_the_staging_directory_changes() {
        // Issue #1154: a run's directory was made under the Staging Directory
        // set when it started. Changing the setting must not strand it.
        let scratch = Scratch::new();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        scratch
            .directories
            .set_root(first.path().to_str().unwrap())
            .unwrap();
        let run = scratch.directories.create("imessage-ios", NOW).unwrap();
        let run_str = run.to_str().unwrap();

        scratch
            .directories
            .set_root(second.path().to_str().unwrap())
            .unwrap();
        // The app restarts with the new setting.
        let directories = scratch.reopen();

        assert_eq!(directories.directory(run_str).unwrap(), run, "resume");
        assert!(directories.openable(run_str).is_ok(), "open the directory");
        // Discard and the clean-up after a finished run both reach this one
        // delete, which no longer looks at the Staging Directory.
        directories.delete(run_str).unwrap();
        assert!(!run.exists(), "discard and clean up");
        let next = directories.create("imessage-ios", NOW).unwrap();
        assert!(next.starts_with(second.path().canonicalize().unwrap()));
    }

    #[test]
    fn a_name_whose_log_is_kept_is_not_given_to_a_new_run() {
        let scratch = Scratch::new();
        let first = scratch.directories.create("imessage-ios", NOW).unwrap();
        let logs = scratch.logs();
        fs::create_dir_all(&logs).unwrap();
        fs::write(import_run_log(&logs, &first), "the first run's log").unwrap();
        // The first run ended and its directory went; its log stays.
        scratch.directories.delete(first.to_str().unwrap()).unwrap();

        let second = scratch.directories.create("imessage-ios", NOW).unwrap();

        assert_ne!(
            import_run_log(&logs, &second),
            import_run_log(&logs, &first)
        );
        assert!(second.ends_with("staging-iphone-ios-261002-101500-2"));
    }

    #[test]
    fn a_new_directory_is_named_for_its_source_and_holds_the_sentinel() {
        let scratch = Scratch::new();

        let run = scratch.directories.create("imessage-ios", NOW).unwrap();
        let again = scratch.directories.create("imessage-ios", NOW).unwrap();

        let root = scratch
            .home
            .path()
            .canonicalize()
            .unwrap()
            .join("message-crate")
            .join("staging");
        assert_eq!(run, root.join("staging-iphone-ios-261002-101500"));
        assert_eq!(again, root.join("staging-iphone-ios-261002-101500-2"));
        assert!(run.join(EXPORT_SENTINEL).is_file());
    }

    /// A run directory holds the conversations it staged until the run ends,
    /// so no other user of the computer may read it, whatever the umask
    /// (#2969). The Staging Directory itself is the person's and keeps its
    /// mode.
    #[cfg(unix)]
    #[test]
    fn a_run_directory_is_its_owner_s_alone() {
        use std::os::unix::fs::PermissionsExt as _;
        let scratch = Scratch::new();
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        scratch
            .directories
            .set_root(root.path().to_str().unwrap())
            .unwrap();

        let run = scratch.directories.create("imessage-ios", NOW).unwrap();

        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&run), 0o700);
        assert_eq!(mode(root.path()), 0o755);
    }

    #[test]
    fn a_label_that_could_leave_the_staging_directory_is_refused() {
        let scratch = Scratch::new();

        for label in ["", "../x", "a/b", "Export"] {
            assert!(scratch.directories.create(label, NOW).is_err(), "{label:?}");
        }
    }

    #[test]
    fn a_directory_this_app_did_not_make_is_refused_even_with_the_sentinel() {
        // A directory the person exported into holds the sentinel too. It is
        // still not a run directory, and must never be deleted as one.
        let scratch = Scratch::new();
        let export = scratch.home.path().join("message-crate").join("my-export");
        fs::create_dir_all(&export).unwrap();
        fs::write(export.join(EXPORT_SENTINEL), "").unwrap();
        let export_str = export.to_str().unwrap();

        let err = scratch.directories.directory(export_str).unwrap_err();
        assert!(err.contains("not a run directory"), "{err}");
        assert!(scratch.reopen().delete(export_str).is_err());
        assert!(scratch.directories.openable(export_str).is_err());
        assert!(export.exists());
    }

    #[test]
    fn a_made_directory_that_holds_files_but_no_sentinel_is_refused() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        fs::write(run.join("notes.txt"), "someone's own").unwrap();
        fs::remove_file(run.join(EXPORT_SENTINEL)).unwrap();

        let err = scratch.reopen().delete(run.to_str().unwrap()).unwrap_err();

        assert!(err.contains(EXPORT_SENTINEL), "{err}");
        assert!(run.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_that_cannot_be_removed_keeps_its_sentinel_and_stays_recorded() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        let locked = run.join("locked");
        fs::create_dir_all(locked.join("inner")).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let directories = scratch.reopen();

        let result = directories.delete(run.to_str().unwrap());

        // Restore permissions so the tempdir can clean itself up.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err(), "a failed removal must not be a quiet Ok");
        // The sentinel goes last, whatever order the disk lists the directory
        // in, so a later delete can finish the job.
        assert!(directories.directory(run.to_str().unwrap()).is_ok());
        directories.delete(run.to_str().unwrap()).unwrap();
        assert!(!run.exists());
    }

    #[test]
    fn deleting_a_directory_already_gone_succeeds_and_forgets_it() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        fs::remove_dir_all(&run).unwrap();

        scratch.reopen().delete(run.to_str().unwrap()).unwrap();

        assert!(scratch.reopen().read().unwrap().directories.is_empty());
    }

    #[test]
    fn a_directory_on_a_drive_that_is_gone_is_not_counted_as_deleted() {
        // The directory it was in is gone too, as when the drive holding the
        // Staging Directory is unplugged: nothing says the directory was deleted.
        let scratch = Scratch::new();
        let drive = tempfile::tempdir().unwrap();
        scratch
            .directories
            .set_root(drive.path().to_str().unwrap())
            .unwrap();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        fs::remove_dir_all(drive.path()).unwrap();

        let err = scratch.reopen().delete(run.to_str().unwrap()).unwrap_err();

        assert!(err.contains("Could not find"), "{err}");
        let recorded = scratch.reopen().read().unwrap().directories;
        assert_eq!(
            recorded.iter().map(|made| &made.path).collect::<Vec<_>>(),
            [&run]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_whose_parent_is_now_on_another_device_is_not_counted_as_deleted() {
        // As when the drive is unplugged and its mount point stays behind as
        // an empty directory: the directory is not found, but nothing deleted it.
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        scratch
            .directories
            .change(|record| {
                for made in &mut record.directories {
                    made.device = made.device.map(|device| device + 1);
                }
                Ok(())
            })
            .unwrap();
        fs::remove_dir_all(&run).unwrap();

        assert!(scratch.directories.delete(run.to_str().unwrap()).is_err());
        assert_eq!(scratch.directories.read().unwrap().directories.len(), 1);
    }

    #[test]
    fn a_delete_that_stopped_after_the_sentinel_went_is_finished() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        fs::remove_file(run.join(EXPORT_SENTINEL)).unwrap();

        scratch.directories.delete(run.to_str().unwrap()).unwrap();

        assert!(!run.exists());
        assert!(scratch.directories.read().unwrap().directories.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_directory_elsewhere_is_removed_and_not_followed() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        fs::write(elsewhere.path().join("keep.txt"), "kept").unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), run.join("link")).unwrap();

        scratch.directories.delete(run.to_str().unwrap()).unwrap();

        assert!(!run.exists());
        assert!(elsewhere.path().join("keep.txt").is_file());
    }

    #[test]
    fn two_processes_keep_each_others_directories() {
        // Two app processes share the file, as two launches or a dev build
        // beside an installed one do. Neither writes over the other.
        let scratch = Scratch::new();
        let other = scratch.reopen();

        let mine = scratch.directories.create("whatsapp", NOW).unwrap();
        let theirs = other.create("imessage-ios", NOW).unwrap();

        assert!(
            scratch
                .directories
                .directory(theirs.to_str().unwrap())
                .is_ok()
        );
        assert!(other.directory(mine.to_str().unwrap()).is_ok());
    }

    #[test]
    fn a_damaged_record_is_an_error_and_is_never_written_over() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        let file = scratch.app_data.path().join(RECORD_FILE);
        fs::write(&file, "{ not json").unwrap();

        let err = scratch.directories.create("whatsapp", NOW).unwrap_err();

        assert!(err.contains("damaged"), "{err}");
        assert!(
            scratch
                .directories
                .directory(run.to_str().unwrap())
                .is_err()
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), "{ not json");
    }

    #[test]
    fn a_file_inside_a_made_directory_is_openable_and_one_beside_it_is_not() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        let log = run.join("message-crate-import.log");
        let beside = run.parent().unwrap().join("notes.txt");
        fs::write(&beside, "").unwrap();

        assert_eq!(
            scratch.directories.openable(log.to_str().unwrap()).unwrap(),
            log
        );
        assert!(
            scratch
                .directories
                .openable(beside.to_str().unwrap())
                .is_err()
        );
        let escape = run.join("..").join("notes.txt");
        assert!(
            scratch
                .directories
                .openable(escape.to_str().unwrap())
                .is_err()
        );
    }

    #[test]
    fn the_staging_directory_is_stored_and_the_default_is_not() {
        let scratch = Scratch::new();
        let directories = &scratch.directories;
        let default_root = scratch.home.path().join("message-crate").join("staging");

        directories.set_root("/data/imports").unwrap();
        let reloaded = scratch.reopen();
        assert_eq!(reloaded.root().unwrap(), PathBuf::from("/data/imports"));
        assert_eq!(
            reloaded.describe().unwrap().default_root,
            default_root.display().to_string()
        );

        directories
            .set_root(default_root.to_str().unwrap())
            .unwrap();
        assert_eq!(directories.read().unwrap().root, None);
        directories.set_root("/data/imports").unwrap();
        directories.set_root("  ").unwrap();
        assert_eq!(directories.root().unwrap(), default_root);
    }

    /// The Message Crate the runs below imported into.
    const CRATE: &str = "3f2a9c";

    /// Start the log of the run whose directory is `run` with the line that
    /// ties it to Import Run `id` by `account` on `message_crate`, as the
    /// window does once the server has created the run.
    fn tie(scratch: &Scratch, run: &Path, id: i64, account: i64, message_crate: &str) {
        RunLog::open(&scratch.logs(), run).account(&message_crate_core::RunLogAccount {
            import_run_id: id,
            account_id: account,
            server: "http://127.0.0.1:8080".to_string(),
            message_crate_id: message_crate.to_string(),
        });
    }

    /// Account 7 on [`CRATE`].
    fn signed_in() -> SignedIn {
        SignedIn {
            account_id: 7,
            message_crate_id: CRATE.to_string(),
        }
    }

    /// Import Run `id` by account 7 on [`CRATE`], whose directory is `run`,
    /// closed as `status`.
    fn closed(run: &Path, id: i64, status: ClosedStatus) -> ClosedRun {
        ClosedRun {
            run_dir: run.display().to_string(),
            import_run_id: id,
            signed_in: signed_in(),
            status,
        }
    }

    /// The bytes of `run`'s sentinel, which every run directory holds.
    fn sentinel_bytes(run: &Path) -> u64 {
        fs::metadata(run.join(EXPORT_SENTINEL)).unwrap().len()
    }

    #[test]
    fn an_account_s_staged_bytes_are_its_own_runs_and_the_whole_is_every_run() {
        // #3087: Settings shows an account the space its own Import Runs take
        // on this computer, and the owner the space every run takes.
        let scratch = Scratch::new();
        let mine = scratch.directories.create("whatsapp", NOW).unwrap();
        let theirs = scratch.directories.create("whatsapp", NOW).unwrap();
        let untied = scratch.directories.create("whatsapp", NOW).unwrap();
        tie(&scratch, &mine, 42, 7, CRATE);
        tie(&scratch, &theirs, 43, 8, CRATE);
        fs::create_dir_all(mine.join("media")).unwrap();
        fs::write(mine.join("media").join("a.jpg"), vec![0; 1000]).unwrap();
        fs::write(mine.join("chat.jsonl"), vec![0; 24]).unwrap();
        fs::write(theirs.join("chat.jsonl"), vec![0; 300]).unwrap();
        fs::write(untied.join("chat.jsonl"), vec![0; 5]).unwrap();
        let sentinels = sentinel_bytes(&mine) + sentinel_bytes(&theirs) + sentinel_bytes(&untied);

        let directories = scratch.reopen();

        assert_eq!(
            directories.bytes_of(&signed_in()).unwrap(),
            1024 + sentinel_bytes(&mine)
        );
        assert_eq!(directories.bytes_of_all().unwrap(), 1329 + sentinels);
    }

    #[test]
    fn the_whole_staging_directory_is_everything_in_it_and_runs_kept_elsewhere() {
        // The owner's total is the whole Staging Directory, a file the person
        // left in it included, and a run still staged under an earlier
        // setting.
        let scratch = Scratch::new();
        let earlier = tempfile::tempdir().unwrap();
        scratch
            .directories
            .set_root(earlier.path().to_str().unwrap())
            .unwrap();
        let kept = scratch.directories.create("whatsapp", NOW).unwrap();
        fs::write(kept.join("chat.jsonl"), vec![0; 70]).unwrap();
        let now = tempfile::tempdir().unwrap();
        scratch
            .directories
            .set_root(now.path().to_str().unwrap())
            .unwrap();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        fs::write(run.join("chat.jsonl"), vec![0; 300]).unwrap();
        fs::write(now.path().join("notes.txt"), vec![0; 9]).unwrap();
        // Beside the earlier run, but not a run: not part of the Staging
        // Directory any more.
        fs::write(earlier.path().join("other.txt"), vec![0; 4000]).unwrap();

        assert_eq!(
            scratch.directories.bytes_of_all().unwrap(),
            379 + sentinel_bytes(&kept) + sentinel_bytes(&run)
        );
    }

    #[test]
    fn a_run_made_inside_another_run_s_directory_counts_once_as_its_own() {
        // The Staging Directory was set inside a run's directory, so the next
        // run was made inside it, and run by another account.
        let scratch = Scratch::new();
        let outer = scratch.directories.create("whatsapp", NOW).unwrap();
        tie(&scratch, &outer, 42, 7, CRATE);
        fs::write(outer.join("chat.jsonl"), vec![0; 100]).unwrap();
        scratch
            .directories
            .set_root(outer.to_str().unwrap())
            .unwrap();
        let inner = scratch.directories.create("imessage-ios", NOW).unwrap();
        tie(&scratch, &inner, 43, 8, CRATE);
        fs::write(inner.join("chat.jsonl"), vec![0; 5000]).unwrap();

        assert_eq!(
            scratch.directories.bytes_of(&signed_in()).unwrap(),
            100 + sentinel_bytes(&outer)
        );
        assert_eq!(
            scratch.directories.bytes_of_all().unwrap(),
            5100 + sentinel_bytes(&outer) + sentinel_bytes(&inner)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_a_run_directory_is_not_measured_through() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        tie(&scratch, &run, 42, 7, CRATE);
        let elsewhere = tempfile::tempdir().unwrap();
        fs::write(elsewhere.path().join("big.bin"), vec![0; 100_000]).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), run.join("link")).unwrap();

        let bytes = scratch.directories.bytes_of(&signed_in()).unwrap();

        assert!(bytes < 100_000, "{bytes}");
    }

    #[test]
    fn a_run_directory_no_longer_on_disk_measures_nothing() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        tie(&scratch, &run, 42, 7, CRATE);
        fs::remove_dir_all(&run).unwrap();

        assert_eq!(scratch.directories.bytes_of(&signed_in()).unwrap(), 0);
        assert_eq!(scratch.directories.bytes_of_all().unwrap(), 0);
    }

    #[test]
    fn the_runs_of_an_account_are_its_own_directories_on_that_message_crate() {
        // #2822: Import cleans up only the directories it can tie to the
        // signed-in account's runs on the Message Crate it is signed in to.
        let scratch = Scratch::new();
        let mine = scratch.directories.create("whatsapp", NOW).unwrap();
        let another_account = scratch.directories.create("whatsapp", NOW).unwrap();
        let another_crate = scratch.directories.create("whatsapp", NOW).unwrap();
        let untied = scratch.directories.create("whatsapp", NOW).unwrap();
        tie(&scratch, &mine, 42, 7, CRATE);
        tie(&scratch, &another_account, 43, 8, CRATE);
        tie(&scratch, &another_crate, 44, 7, "another");

        let runs = scratch.reopen().runs_of(&signed_in()).unwrap();

        assert_eq!(
            runs,
            [DirectoryOfRun {
                run_dir: mine.display().to_string(),
                import_run_id: 42,
            }]
        );
        assert!(untied.exists());
    }

    #[test]
    fn a_closed_run_s_directory_is_deleted_and_its_log_says_so() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        tie(&scratch, &run, 42, 7, CRATE);

        let deleted = scratch
            .reopen()
            .delete_closed(&closed(&run, 42, ClosedStatus::Canceled))
            .unwrap();

        assert!(deleted);
        assert!(!run.exists());
        assert!(scratch.reopen().runs_of(&signed_in()).unwrap().is_empty());
        let log = std::fs::read_to_string(import_run_log(&scratch.logs(), &run)).unwrap();
        let last = log.lines().last().unwrap();
        assert!(last.contains(" INFO "), "{last}");
        assert!(
            last.contains(&format!(
                "Deleted the directory of Import Run 42, {}, because the server reports the run as canceled",
                run.display()
            )),
            "{last}"
        );
    }

    #[test]
    fn a_closed_run_s_directory_already_gone_is_forgotten_without_a_log_line() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        tie(&scratch, &run, 42, 7, CRATE);
        fs::remove_dir_all(&run).unwrap();
        let log = import_run_log(&scratch.logs(), &run);
        let before = fs::read_to_string(&log).unwrap();

        let deleted = scratch
            .directories
            .delete_closed(&closed(&run, 42, ClosedStatus::Completed))
            .unwrap();

        assert!(!deleted);
        assert!(scratch.directories.read().unwrap().directories.is_empty());
        assert_eq!(fs::read_to_string(&log).unwrap(), before);
    }

    #[test]
    fn a_directory_whose_log_names_another_run_is_not_deleted_as_closed() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        let untied = scratch.directories.create("whatsapp", NOW).unwrap();
        tie(&scratch, &run, 42, 7, CRATE);

        let err = scratch
            .directories
            .delete_closed(&closed(&run, 41, ClosedStatus::Failed))
            .unwrap_err();
        assert!(err.contains("not the directory of Import Run 41"), "{err}");
        assert!(
            scratch
                .directories
                .delete_closed(&closed(&untied, 42, ClosedStatus::Failed))
                .is_err()
        );

        assert!(run.exists());
        assert!(untied.exists());
    }

    #[test]
    fn a_run_of_the_same_id_on_another_message_crate_keeps_its_directory() {
        // Import Run ids are counted per Message Crate. Run 42 on another
        // Message Crate may still be running there, whatever this one says
        // about its own run 42.
        let scratch = Scratch::new();
        let run = scratch.directories.create("whatsapp", NOW).unwrap();
        tie(&scratch, &run, 42, 7, "another");

        let err = scratch
            .directories
            .delete_closed(&closed(&run, 42, ClosedStatus::Canceled))
            .unwrap_err();

        assert!(err.contains("not the directory of Import Run 42"), "{err}");
        assert!(run.exists());
    }

    #[test]
    fn a_closed_status_is_written_in_the_log_as_the_server_writes_it() {
        for status in [
            ClosedStatus::Completed,
            ClosedStatus::CompletedWithIssues,
            ClosedStatus::Failed,
            ClosedStatus::Canceled,
        ] {
            let quoted = format!("\"{}\"", status.as_str());
            assert_eq!(
                serde_json::from_str::<ClosedStatus>(&quoted).unwrap(),
                status
            );
        }
    }

    #[test]
    fn running_is_not_a_closed_status() {
        assert!(serde_json::from_str::<ClosedStatus>("\"running\"").is_err());
        assert_eq!(
            serde_json::from_str::<ClosedStatus>("\"completed_with_issues\"").unwrap(),
            ClosedStatus::CompletedWithIssues
        );
    }

    #[test]
    fn a_relative_or_filesystem_root_staging_directory_is_refused() {
        let scratch = Scratch::new();

        assert!(scratch.directories.set_root("message-crate").is_err());
        assert!(scratch.directories.set_root("/").is_err());
        assert_eq!(scratch.directories.read().unwrap().root, None);
    }
}
