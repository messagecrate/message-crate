//! The Staging Directory and the run directories this app made under it.
//!
//! The desktop process owns both. The window never names a root:
//! `set_staging_root` stores the Staging Directory here, `create_staging_dir`
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

use message_ir_format::{EXPORT_SENTINEL, mark_export_directory};

use crate::commands::paths::{resolve_openable_path, resolve_staging_root};

/// File in the app-data directory that holds the Staging Directory setting
/// and the run directories this app made.
pub const RECORD_FILE: &str = "staging.json";

/// File beside [`RECORD_FILE`] that every process locks to read or change it.
const LOCK_FILE: &str = "staging.json.lock";

/// Directory under the home directory that is the Staging Directory when Settings
/// name none.
const DEFAULT_ROOT_NAME: &str = "message-crate";

/// What [`RECORD_FILE`] holds.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Record {
    /// The Staging Directory from Settings. `None` means the default,
    /// `{home}/message-crate`.
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
    /// `{home}/message-crate`: the directory used when Settings name none.
    pub default_root: String,
}

/// The Staging Directory and the run directories made under it, kept in
/// [`RECORD_FILE`].
#[derive(Debug)]
pub struct StagingDirectories {
    /// Where the record is saved.
    file: PathBuf,
    /// The home directory, for the default Staging Directory. `None` when the
    /// operating system reports none.
    home: Option<PathBuf>,
}

impl StagingDirectories {
    /// The record kept in `file`, read from it on every use.
    pub fn at(file: PathBuf, home: Option<PathBuf>) -> Self {
        Self { file, home }
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

    /// `{home}/message-crate`.
    fn default_root(&self) -> Result<PathBuf, String> {
        self.home
            .as_ref()
            .map(|home| home.join(DEFAULT_ROOT_NAME))
            .ok_or_else(|| "Could not determine the user home directory".to_string())
    }

    /// The Staging Directory new run directories are made in.
    ///
    /// # Errors
    ///
    /// Returns an error when Settings name no directory and the operating
    /// system reports no home directory.
    pub fn root(&self) -> Result<PathBuf, String> {
        match self.read()?.root {
            Some(root) => Ok(root),
            None => self.default_root(),
        }
    }

    /// The Staging Directory and its default, for Settings.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating system reports no home directory, or
    /// the record cannot be read.
    pub fn describe(&self) -> Result<StagingRoot, String> {
        Ok(StagingRoot {
            root: self.root()?.display().to_string(),
            default_root: self.default_root()?.display().to_string(),
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
            if self.default_root().ok().as_ref() == Some(&path) {
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
    /// `label` names what the directory is for: an Import source such as
    /// `imessage-ios`, or `export`.
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
            match std::fs::create_dir(&directory) {
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
    /// not made by [`StagingDirectories::create`], or has no sentinel.
    pub fn directory(&self, dir: &str) -> Result<PathBuf, String> {
        let path = absolute(dir)?;
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
    /// Returns an error when the directory fails [`StagingDirectories::directory`]'s
    /// checks, cannot be removed, or the record cannot be saved.
    pub fn delete(&self, dir: &str) -> Result<(), String> {
        let path = absolute(dir)?;
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

    /// Drop `directory` from the record.
    fn forget(&self, directory: &Path) -> Result<(), String> {
        self.change(|record| {
            record.directories.retain(|made| made.path != directory);
            Ok(())
        })
    }

    /// Resolve `path` for opening: a run directory this app made, or a file
    /// or directory inside one, such as its push log.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is empty or relative, or is in no
    /// run directory this app made.
    pub fn openable(&self, path: &str) -> Result<PathBuf, String> {
        absolute(path)?;
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

/// `raw`, trimmed, when it is a non-empty absolute path.
fn absolute(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Path is empty".to_string());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err("Path must be absolute".to_string());
    }
    Ok(path)
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
/// directories, for tests here and in the staging commands.
#[cfg(test)]
pub(crate) struct Scratch {
    pub directories: StagingDirectories,
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
    fn open(app_data: &tempfile::TempDir, home: &tempfile::TempDir) -> StagingDirectories {
        StagingDirectories::at(
            app_data.path().join(RECORD_FILE),
            Some(home.path().to_path_buf()),
        )
    }

    /// The same record, as another process or a restarted app opens it.
    pub fn reopen(&self) -> StagingDirectories {
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
    fn a_new_directory_is_named_for_its_source_and_holds_the_sentinel() {
        let scratch = Scratch::new();

        let run = scratch.directories.create("imessage-ios", NOW).unwrap();
        let again = scratch.directories.create("imessage-ios", NOW).unwrap();

        let root = scratch
            .home
            .path()
            .canonicalize()
            .unwrap()
            .join("message-crate");
        assert_eq!(run, root.join("staging-iphone-ios-261002-101500"));
        assert_eq!(again, root.join("staging-iphone-ios-261002-101500-2"));
        assert!(run.join(EXPORT_SENTINEL).is_file());
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
        let run = scratch.directories.create("export", NOW).unwrap();
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
        let run = scratch.directories.create("export", NOW).unwrap();
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
        let run = scratch.directories.create("export", NOW).unwrap();
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
        let run = scratch.directories.create("export", NOW).unwrap();
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
        let run = scratch.directories.create("export", NOW).unwrap();
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
        let run = scratch.directories.create("export", NOW).unwrap();
        fs::remove_file(run.join(EXPORT_SENTINEL)).unwrap();

        scratch.directories.delete(run.to_str().unwrap()).unwrap();

        assert!(!run.exists());
        assert!(scratch.directories.read().unwrap().directories.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_directory_elsewhere_is_removed_and_not_followed() {
        let scratch = Scratch::new();
        let run = scratch.directories.create("export", NOW).unwrap();
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

        let mine = scratch.directories.create("export", NOW).unwrap();
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
        let run = scratch.directories.create("export", NOW).unwrap();
        let file = scratch.app_data.path().join(RECORD_FILE);
        fs::write(&file, "{ not json").unwrap();

        let err = scratch.directories.create("export", NOW).unwrap_err();

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
        let run = scratch.directories.create("export", NOW).unwrap();
        let log = run.join("message-crate-push.log");
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
        let default_root = scratch.home.path().join("message-crate");

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

    #[test]
    fn a_relative_or_filesystem_root_staging_directory_is_refused() {
        let scratch = Scratch::new();

        assert!(scratch.directories.set_root("message-crate").is_err());
        assert!(scratch.directories.set_root("/").is_err());
        assert_eq!(scratch.directories.read().unwrap().root, None);
    }
}
