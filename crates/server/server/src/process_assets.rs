//! Make the Thumbnail and the Preview of stored attachments under
//! `assets_converted/` (`docs/architecture/media.md`, rules 3 and 4).
//!
//! Every image and video gets a Thumbnail, and every image, video or audio
//! file of a type a browser often cannot show gets a Preview
//! ([`media::browser_shows`]). Originals are never changed. Each version is
//! stored under the fingerprint of its own bytes and named by the
//! attachment rows of its original (`attachments.thumbnail_*` and
//! `attachments.derived_*`). The conversions are the `media` crate's, which
//! finds ffmpeg and ffprobe beside the binary, in `MESSAGE_CRATE_BIN`, or on
//! `PATH`.
//!
//! Two callers run it: the `process-assets` command over every attachment,
//! for rebuilding and repair, and the server's background pass over the
//! Assets an Import Run brought ([`crate::media_queue`]).

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use sqlx::{SqliteConnection, SqlitePool};
use tempfile::TempDir;

use crate::config::Config;
use crate::db::attachment_versions::{self as versions_db, StoredOriginal, Version, VersionFile};
use crate::db::{account_profile, schema};
use crate::open_db::OpenDb;
use media::Kind;

/// Options for one processing pass.
#[derive(Debug, Clone, Default)]
pub struct ProcessAssetsOptions {
    /// Make every Thumbnail and Preview again, even one that exists and
    /// hashes to the fingerprint in its name. A Preview of an original that
    /// every browser shows is kept, not made again.
    pub force: bool,
    /// Convert and log without writing files or updating the database.
    pub dry_run: bool,
    /// Leave images alone.
    pub skip_image: bool,
    /// Leave videos alone.
    pub skip_video: bool,
    /// Leave audio alone.
    pub skip_audio: bool,
    /// Only process this account. `None` processes every account, which is
    /// what the `process-assets` command does. A Demo Account build names
    /// the Demo Account, so it converts nothing of any other account and
    /// never sweeps another account's upload temps.
    pub account: Option<i64>,
}

/// Counts reported by one processing pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProcessAssetsStats {
    /// Stored originals examined.
    pub scanned: u64,
    /// Previews written (JPEG/MP4/MP3).
    pub derived: u64,
    /// Thumbnails written.
    pub thumbnails: u64,
    /// Originals for which nothing was written: not media, already done, or
    /// of a kind the options leave alone.
    pub skipped: u64,
    /// Originals for which a version could not be made.
    pub errors: u64,
}

impl ProcessAssetsStats {
    /// Add the counts of another pass.
    pub(crate) fn add(&mut self, other: &Self) {
        self.scanned += other.scanned;
        self.derived += other.derived;
        self.thumbnails += other.thumbnails;
        self.skipped += other.skipped;
        self.errors += other.errors;
    }

    /// Count what processing one original did.
    fn count(&mut self, outcome: &Outcome) {
        self.scanned += 1;
        if outcome.preview {
            self.derived += 1;
        }
        if outcome.thumbnail {
            self.thumbnails += 1;
        }
        if outcome.error.is_some() {
            self.errors += 1;
        } else if !outcome.preview && !outcome.thumbnail {
            self.skipped += 1;
        }
    }
}

/// Where a pass says what it does: on standard output, for the
/// `process-assets` command, or in the server's log, for the background
/// pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Log {
    Print,
    Trace,
}

impl Log {
    fn say(self, line: impl fmt::Display) {
        match self {
            Self::Print => println!("{line}"),
            Self::Trace => tracing::info!("{line}"),
        }
    }

    fn fail(self, line: impl fmt::Display) {
        match self {
            Self::Print => eprintln!("{line}"),
            Self::Trace => tracing::warn!("{line}"),
        }
    }
}

/// The original's media type, from everything its rows know about it.
fn media_type(row: &StoredOriginal) -> Option<String> {
    media::media_type_of(
        Path::new(&row.assets_path),
        row.mime_type.as_deref(),
        &row.name_hints(),
    )
}

/// Make the versions of every stored original of the account `opts` names,
/// or of every account in the database when it names none, saying what it
/// does on standard output. An account's attachments from every source share
/// one originals directory and one converted directory, so each account is
/// one pass.
///
/// # Errors
///
/// Returns an error when no account is named and the database has none, a
/// query fails, or an account's asset directories cannot be prepared. A
/// conversion that fails for one attachment is counted in `errors` and
/// printed, and the run goes on.
pub async fn run(
    opened: &OpenDb,
    opts: &ProcessAssetsOptions,
    stop: &AtomicBool,
) -> Result<ProcessAssetsStats> {
    let cfg = &opened.cfg;
    let account_ids = match opts.account {
        Some(account_id) => vec![account_id],
        None => list_account_ids(&mut *opened.conn().await?, &cfg.paths.data_dir).await?,
    };
    if account_ids.is_empty() {
        bail!("no accounts found — create an account or run reset-demo first");
    }

    let work = work_dir(&cfg.paths.data_dir)?;
    let mut stats = ProcessAssetsStats::default();

    for &account_id in &account_ids {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let Some(pass) = AccountPass::open(cfg, opts, work.path(), account_id, stop)? else {
            continue;
        };
        let rows =
            versions_db::stored_originals(&mut *opened.conn().await?, account_id, None).await?;
        stats.add(&pass.process_rows(&opened.db, &rows).await);
    }
    if stop.load(Ordering::Relaxed) {
        // The work directory, with what the stopped conversion wrote, is
        // removed as `work` drops.
        bail!(
            "stopped before every attachment was processed; what was made is kept, and running \
             process-assets again makes the rest"
        );
    }

    println!(
        "done: scanned={} converted_for_web={} thumbnails={} left_as_is={} conversion_failures={}{}",
        stats.scanned,
        stats.derived,
        stats.thumbnails,
        stats.skipped,
        stats.errors,
        if opts.dry_run { " (dry-run)" } else { "" }
    );
    Ok(stats)
}

/// Make the versions of one stored original, `sha256` of `account_id`, as
/// the background pass does after an Import Run, saying what it does in the
/// server's log. Unlike [`run`] it sweeps nothing, because it runs once for
/// each Asset an Import Run brought. An account with no originals directory,
/// or an original no attachment names any more, is nothing to do. Setting
/// `stop` kills the conversion that runs and starts no other; the part-made
/// file is removed.
///
/// # Errors
///
/// Returns an error when a query fails or the converted directory cannot be
/// made. A version that cannot be made is counted in `errors` and logged.
pub(crate) async fn process_one_asset(
    cfg: &Config,
    db: &SqlitePool,
    work_dir: &Path,
    account_id: i64,
    sha256: &str,
    stop: &AtomicBool,
) -> Result<ProcessAssetsStats> {
    let opts = ProcessAssetsOptions::default();
    let Some(pass) = AccountPass::new(cfg, &opts, work_dir, account_id, stop, Log::Trace)? else {
        return Ok(ProcessAssetsStats::default());
    };
    let rows =
        versions_db::stored_originals(&mut *db.acquire().await?, account_id, Some(sha256)).await?;
    Ok(pass.process_rows(db, &rows).await)
}

/// The directory under `data_dir` that holds the work directories of every
/// pass, `process-assets` and the server's alike.
const WORK_DIRS: &str = ".media-work";

/// Age after which a work directory under [`WORK_DIRS`] is left over from a
/// pass that was stopped: no pass spends a day on one file.
const STALE_WORK_SECS: u64 = 24 * 60 * 60;

/// A new work directory for one pass under `data_dir/.media-work/`, removed
/// when the pass is done. A pass stopped part-way, by a server stopped or
/// killed, cannot remove its own, so the work directories older than a day
/// are removed first. They hold part-made copies of attachments, and live
/// beside the data rather than in the system's temporary directory so they
/// are never left where nothing looks again.
///
/// # Errors
///
/// Returns an error when the directory cannot be made.
pub(crate) fn work_dir(data_dir: &Path) -> Result<TempDir> {
    let root = data_dir.join(WORK_DIRS);
    fs::create_dir_all(&root).with_context(|| format!("create {}", root.display()))?;
    let now = std::time::SystemTime::now();
    for entry in fs::read_dir(&root).into_iter().flatten().flatten() {
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .is_ok_and(|modified| {
                now.duration_since(modified)
                    .is_ok_and(|age| age.as_secs() >= STALE_WORK_SECS)
            });
        if stale && let Err(error) = fs::remove_dir_all(entry.path()) {
            tracing::warn!(
                path = %entry.path().display(),
                %error,
                "a work directory a stopped pass left could not be removed"
            );
        }
    }
    tempfile::Builder::new()
        .prefix("pass-")
        .tempdir_in(&root)
        .with_context(|| format!("make a work directory in {}", root.display()))
}

/// What processing one stored original did.
#[derive(Debug, Default)]
struct Outcome {
    /// A Preview was written, or would be in a dry run.
    preview: bool,
    /// A Thumbnail was written, or would be in a dry run.
    thumbnail: bool,
    /// Why a version that was wanted was not made.
    error: Option<anyhow::Error>,
}

/// One account's asset directories being processed: where its originals
/// are, where the versions go, and the options every attachment shares.
struct AccountPass<'a> {
    opts: &'a ProcessAssetsOptions,
    work_dir: &'a Path,
    /// Set to stop the pass: the conversion that runs is killed, and no
    /// other starts.
    stop: &'a AtomicBool,
    account_id: i64,
    assets_dir: PathBuf,
    converted_dir: PathBuf,
    log: Log,
}

/// What making one version produced.
#[derive(Debug, PartialEq, Eq)]
enum Derived {
    /// A dry run: said what it would write and stored nothing.
    DryRun,
    Stored(VersionFile),
}

impl<'a> AccountPass<'a> {
    /// Find the directories, remove abandoned upload temps and the temporary
    /// files a killed write left in the shard directories, and make the
    /// converted directory. `None` when the account has no assets directory
    /// to process.
    ///
    /// # Errors
    ///
    /// Returns an error when the converted directory cannot be made. A
    /// failed removal of a temporary file is logged and does not stop the
    /// pass.
    fn open(
        cfg: &Config,
        opts: &'a ProcessAssetsOptions,
        work_dir: &'a Path,
        account_id: i64,
        stop: &'a AtomicBool,
    ) -> Result<Option<Self>> {
        let assets_dir = cfg.paths.assets_dir_for_account(account_id);
        let converted_dir = cfg.paths.assets_converted_dir_for_account(account_id);
        println!("account {account_id}: assets={}", assets_dir.display());
        if !assets_dir.is_dir() {
            eprintln!("  skip — assets dir missing");
            return Ok(None);
        }
        let cleaned_verb = if opts.dry_run {
            "would clean"
        } else {
            "cleaned"
        };
        let cleaned = crate::asset_store::sweep_incoming(&assets_dir, opts.dry_run);
        if cleaned > 0 {
            println!("  {cleaned_verb} {cleaned} abandoned upload temp(s) under .incoming/");
        }
        let left = crate::asset_store::sweep_shard_temps(&assets_dir, opts.dry_run)
            + crate::asset_store::sweep_shard_temps(&converted_dir, opts.dry_run);
        if left > 0 {
            println!(
                "  {cleaned_verb} {left} temporary file(s) a killed write left in the shard directories"
            );
        }
        Self::new(cfg, opts, work_dir, account_id, stop, Log::Print)
    }

    /// The pass over `account_id`'s directories, making the converted
    /// directory, and sweeping nothing. `None` when the account has no
    /// assets directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the converted directory cannot be made.
    fn new(
        cfg: &Config,
        opts: &'a ProcessAssetsOptions,
        work_dir: &'a Path,
        account_id: i64,
        stop: &'a AtomicBool,
        log: Log,
    ) -> Result<Option<Self>> {
        let assets_dir = cfg.paths.assets_dir_for_account(account_id);
        if !assets_dir.is_dir() {
            return Ok(None);
        }
        let converted_dir = cfg.paths.assets_converted_dir_for_account(account_id);
        fs::create_dir_all(&converted_dir)
            .with_context(|| format!("create converted dir {}", converted_dir.display()))?;
        Ok(Some(Self {
            opts,
            work_dir,
            stop,
            account_id,
            assets_dir,
            converted_dir,
            log,
        }))
    }

    /// `account/path`: how log lines name an attachment.
    fn label(&self, row: &StoredOriginal) -> String {
        format!("{}/{}", self.account_id, row.assets_path)
    }

    /// Process each of `rows` and count what happened, logging each failure.
    async fn process_rows(&self, db: &SqlitePool, rows: &[StoredOriginal]) -> ProcessAssetsStats {
        let mut stats = ProcessAssetsStats::default();
        for row in rows {
            if self.stop.load(Ordering::Relaxed) {
                break;
            }
            // A work directory whose time is a day old counts as left by a
            // stopped pass, so a live one is touched before each original.
            let _ = fs::File::open(self.work_dir)
                .and_then(|dir| dir.set_modified(std::time::SystemTime::now()));
            let outcome = self.process(db, row).await;
            if outcome.error.is_some() && self.stop.load(Ordering::Relaxed) {
                // A conversion the stop killed is not a failure: it is made
                // again by the next pass.
                break;
            }
            if let Some(err) = &outcome.error {
                self.log
                    .fail(format!("failed {}: {err:#}", self.label(row)));
            }
            stats.count(&outcome);
        }
        stats
    }

    /// Make what one stored original still needs, or say why nothing.
    ///
    /// Reads the facts only the disk can supply, lets [`plan`] decide, then
    /// does what the plan says. An existing version is hashed, so one cut
    /// short by a killed run is made again.
    async fn process(&self, db: &SqlitePool, row: &StoredOriginal) -> Outcome {
        let source_path = self.assets_dir.join(&row.assets_path);
        let state = |version: Version| {
            preview_file(
                row.named(version).assets_path.as_deref(),
                &self.converted_dir,
            )
        };
        let on_disk = OnDisk {
            original_exists: source_path.is_file(),
            preview: state(Version::Preview),
            thumbnail: state(Version::Thumbnail),
            browser_shows: media::browser_shows(&source_path, media_type(row).as_deref()),
        };
        let versions = match plan(row, self.opts, on_disk) {
            Plan::RemoveIncomplete => {
                return Outcome {
                    error: self.remove_incomplete(row, &source_path).err(),
                    ..Outcome::default()
                };
            }
            Plan::Skip(_) => return Outcome::default(),
            Plan::Versions(versions) => versions,
        };
        let mut outcome = Outcome::default();
        let mut failures = Vec::new();
        for (version, need, made) in [
            (
                Version::Thumbnail,
                versions.thumbnail,
                &mut outcome.thumbnail,
            ),
            (Version::Preview, versions.preview, &mut outcome.preview),
        ] {
            let damaged = match version {
                Version::Preview => on_disk.preview,
                Version::Thumbnail => on_disk.thumbnail,
            } == PreviewFile::Damaged;
            let done = match need {
                Need::Nothing => Ok(false),
                Need::Share => self.share_existing(db, row, version).await.map(|()| false),
                Need::Drop => self.drop_damaged(db, row, version).await.map(|()| false),
                Need::NoOriginal { damaged } => {
                    let dropped = if damaged {
                        self.drop_damaged(db, row, version).await
                    } else {
                        Ok(())
                    };
                    dropped.and(Err(anyhow::anyhow!("missing original")))
                }
                Need::Make => {
                    self.make(db, row, version, versions.kind, &source_path, damaged)
                        .await
                }
            };
            match done {
                Ok(written) => *made = written,
                Err(err) => failures.push(format!("{version}: {err:#}")),
            }
        }
        if !failures.is_empty() {
            outcome.error = Some(anyhow::anyhow!(failures.join("; ")));
        }
        outcome
    }

    /// Make `version` of the original at `source_path`, a `kind` file, store
    /// it and point every row of the original at it. True when it was
    /// written, or would be in a dry run.
    async fn make(
        &self,
        db: &SqlitePool,
        row: &StoredOriginal,
        version: Version,
        kind: Kind,
        source_path: &Path,
        damaged: bool,
    ) -> Result<bool> {
        if damaged && let Some(rel) = row.named(version).assets_path.as_deref() {
            self.log.say(format!(
                "{}: {version} {rel} does not hash to the fingerprint in its name; making it again",
                self.label(row)
            ));
        }
        let blob = match self.derive(version, kind, source_path, row)? {
            Derived::DryRun => return Ok(true),
            Derived::Stored(blob) => blob,
        };
        let named = versions_db::record(
            &mut *db.acquire().await?,
            version,
            self.account_id,
            &row.sha256,
            &blob,
        )
        .await?;
        if named == 0 {
            // Every row of the original was deleted while the version was
            // made, so the delete could not report the file. It is left for
            // the sweep at the next Import Run's end, which keeps a young
            // file: the same bytes may be the version of another original
            // that a concurrent pass is about to record.
            self.log.say(format!(
                "{}: deleted while its {version} was made; the {version} is left for the sweep",
                self.label(row)
            ));
            return Ok(false);
        }
        self.log.say(format!(
            "{} -> {} ({version})",
            self.label(row),
            blob.assets_path
        ));
        Ok(true)
    }

    /// Point the rows of `row`'s original that name no `version` at the one
    /// the other rows already name. A file imported from a second source
    /// after its versions were made has such rows; without this they would
    /// never say a version exists, because the original is not converted
    /// again.
    ///
    /// # Errors
    ///
    /// Returns an error when the rows cannot be updated.
    async fn share_existing(
        &self,
        db: &SqlitePool,
        row: &StoredOriginal,
        version: Version,
    ) -> Result<()> {
        let named = row.named(version);
        if named.rows_without == 0 {
            return Ok(());
        }
        let (Some(sha256), Some(assets_path), Some(mime_type)) =
            (named.sha256, named.assets_path, named.mime_type)
        else {
            return Ok(());
        };
        let blob = VersionFile {
            sha256,
            assets_path,
            mime_type,
        };
        if self.opts.dry_run {
            self.log.say(format!(
                "[dry-run] would point {} at {} (existing {version})",
                self.label(row),
                blob.assets_path
            ));
            return Ok(());
        }
        versions_db::record(
            &mut *db.acquire().await?,
            version,
            self.account_id,
            &row.sha256,
            &blob,
        )
        .await?;
        self.log.say(format!(
            "{} -> {} (existing {version})",
            self.label(row),
            blob.assets_path
        ));
        Ok(())
    }

    /// Stop naming the damaged `version` that `row` names and delete it, or
    /// say so in a dry run. It is not made again, because its original is
    /// missing or the original no longer gets such a version, and the rows
    /// must not go on naming a file the server would serve as if whole. Every row of the account that names it is cleared,
    /// then the file is deleted.
    ///
    /// # Errors
    ///
    /// Returns an error when the rows cannot be updated or the file cannot
    /// be deleted.
    async fn drop_damaged(
        &self,
        db: &SqlitePool,
        row: &StoredOriginal,
        version: Version,
    ) -> Result<()> {
        let Some(rel) = row.named(version).assets_path else {
            return Ok(());
        };
        if self.opts.dry_run {
            self.log.say(format!(
                "[dry-run] would drop the damaged {version} {rel} of {}",
                self.label(row)
            ));
            return Ok(());
        }
        versions_db::clear(&mut *db.acquire().await?, version, self.account_id, &rel).await?;
        if let Some(path) = crate::asset_store::join_under(&self.converted_dir, &rel)
            && path.is_file()
        {
            crate::asset_store::remove_file(&path)
                .with_context(|| format!("remove damaged {version} {}", path.display()))?;
        }
        self.log.say(format!(
            "{}: {version} {rel} does not hash to the fingerprint in its name and is not made \
             again; dropped the {version}",
            self.label(row)
        ));
        Ok(())
    }

    /// Delete a `.part` left by an interrupted upload, or say so in a dry
    /// run.
    fn remove_incomplete(&self, row: &StoredOriginal, source_path: &Path) -> Result<()> {
        if source_path.is_file() {
            if self.opts.dry_run {
                self.log.say(format!(
                    "[dry-run] would remove incomplete {}",
                    self.label(row)
                ));
            } else {
                crate::asset_store::remove_file(source_path)
                    .with_context(|| format!("remove incomplete {}", source_path.display()))?;
                self.log
                    .say(format!("removed incomplete {}", self.label(row)));
            }
        }
        Ok(())
    }

    /// Make `version` of one original into the work directory, then store
    /// it.
    fn derive(
        &self,
        version: Version,
        kind: Kind,
        source_path: &Path,
        row: &StoredOriginal,
    ) -> Result<Derived> {
        let ext = match version {
            Version::Thumbnail => ".jpg",
            Version::Preview => media::preview_extension(kind),
        };
        let token = row.sha256.get(..12).unwrap_or(&row.sha256);
        let out = self.work_dir.join(format!("{version}-{token}{ext}"));
        let made = match version {
            Version::Thumbnail => media::make_thumbnail(source_path, &out, self.stop),
            Version::Preview => media::make_preview(source_path, kind, &out, self.stop),
        };
        if let Err(err) = made {
            let _ = fs::remove_file(&out);
            return Err(err);
        }
        self.store_work_file(&out, version, ext, row)
    }

    /// Store a version the media pass wrote to the work directory, then
    /// remove the work file.
    fn store_work_file(
        &self,
        out: &Path,
        version: Version,
        ext: &str,
        row: &StoredOriginal,
    ) -> Result<Derived> {
        if self.opts.dry_run {
            self.log
                .say(format!("[dry-run] {version} {} -> {ext}", self.label(row)));
            let _ = fs::remove_file(out);
            return Ok(Derived::DryRun);
        }
        // A deleted account's directory is gone, and storing would make it
        // again with nothing to remove it.
        if !self.assets_dir.is_dir() {
            let _ = fs::remove_file(out);
            bail!("the account's directory is gone");
        }
        let blob = store_derived_file(&self.converted_dir, out, ext);
        let _ = fs::remove_file(out);
        Ok(Derived::Stored(blob?))
    }
}

/// Account ids from the database, falling back to the directory names under
/// `data_dir` when the table does not exist yet.
async fn list_account_ids(conn: &mut SqliteConnection, data_dir: &Path) -> Result<Vec<i64>> {
    let mut ids = Vec::new();
    if schema::table_exists(conn, "accounts").await? {
        ids = account_profile::account_ids(conn).await?;
    }
    if ids.is_empty() && data_dir.is_dir() {
        // Account directories are named by id; anything else under `data/`
        // is not an account.
        for entry in fs::read_dir(data_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && let Ok(id) = entry.file_name().to_string_lossy().parse::<i64>()
            {
                ids.push(id);
            }
        }
        ids.sort_unstable();
    }
    Ok(ids)
}

/// Incomplete iMessage/SMS transfers and aborted uploads use a `.part` suffix.
fn is_part_path(path: &str) -> bool {
    crate::asset_store::has_part_extension(Path::new(path))
}

/// What one stored original needs, decided before any file is touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    /// A `.part` left by an interrupted transfer: delete it, never hand it to ffmpeg.
    RemoveIncomplete,
    /// Leave the original and its versions as they are.
    Skip(SkipReason),
    /// What each version needs.
    Versions(Versions),
}

/// What the two versions of one media original need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Versions {
    /// The kind of media the original is.
    kind: Kind,
    thumbnail: Need,
    preview: Need,
}

/// What one version of an original needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    /// Nothing: this original gets no such version, or it has one that its
    /// rows all name.
    Nothing,
    /// An intact one exists: point the rows that name none at it.
    Share,
    /// Make it from the original.
    Make,
    /// This original gets no such version, and the rows name one that is
    /// damaged: stop naming it and delete it.
    Drop,
    /// It must be made and the original is missing: count the original as a
    /// failure. When the version on disk is damaged it cannot be made again,
    /// so stop naming it and delete it first.
    NoOriginal { damaged: bool },
}

/// Why an original is left as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkipReason {
    /// Not an image, video or audio file.
    NotMedia,
    /// The options turn this kind off (`--skip-image` and friends).
    KindDisabled,
}

/// The facts about one original that only the filesystem can supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OnDisk {
    /// The stored original is present under the assets directory.
    original_exists: bool,
    /// The state of the Preview the rows name.
    preview: PreviewFile,
    /// The state of the Thumbnail the rows name.
    thumbnail: PreviewFile,
    /// Every browser shows the original as it is ([`media::browser_shows`]),
    /// so it needs no Preview.
    browser_shows: bool,
}

/// The state of a version a row names, under the converted directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewFile {
    /// None is named, or no file is at the path named.
    Missing,
    /// A file is there, but its bytes do not hash to the fingerprint in its
    /// name, such as one cut short by a killed run. It is made again as if
    /// missing, or dropped when its original is missing.
    Damaged,
    /// A file is there and its bytes hash to the fingerprint in its name.
    Intact,
}

/// Decide what one original needs from the row, the options and what is on
/// disk. No IO happens here: `on_disk` carries the facts the caller read.
fn plan(row: &StoredOriginal, opts: &ProcessAssetsOptions, on_disk: OnDisk) -> Plan {
    if is_part_path(&row.assets_path) {
        return Plan::RemoveIncomplete;
    }
    // A GIF is an animation, so it gets no still Preview, but it is an image
    // and gets a Thumbnail like any other.
    let preview_kind = media::kind_of(
        Path::new(&row.assets_path),
        row.mime_type.as_deref(),
        &row.name_hints(),
    );
    let is_gif = media_type(row).as_deref() == Some("image/gif");
    let Some(kind) = preview_kind.or(is_gif.then_some(Kind::Image)) else {
        return Plan::Skip(SkipReason::NotMedia);
    };
    let wanted = match kind {
        Kind::Image => !opts.skip_image,
        Kind::Video => !opts.skip_video,
        Kind::Audio => !opts.skip_audio,
    };
    if !wanted {
        return Plan::Skip(SkipReason::KindDisabled);
    }
    // An intact version is kept, made again only under `--force`, and one
    // this original would no longer get is kept even then.
    let need = |wanted: bool, state: PreviewFile| {
        if state == PreviewFile::Intact && !(opts.force && wanted) {
            Need::Share
        } else if !wanted {
            if state == PreviewFile::Damaged {
                Need::Drop
            } else {
                Need::Nothing
            }
        } else if on_disk.original_exists {
            Need::Make
        } else {
            Need::NoOriginal {
                damaged: state == PreviewFile::Damaged,
            }
        }
    };
    Plan::Versions(Versions {
        kind,
        thumbnail: need(matches!(kind, Kind::Image | Kind::Video), on_disk.thumbnail),
        preview: need(
            preview_kind.is_some() && !on_disk.browser_shows,
            on_disk.preview,
        ),
    })
}

/// The state of the version `assets_path` names under `converted_dir`. A
/// path that is empty or leaves `converted_dir` names no file there, and a
/// name that carries no fingerprint, which the server never writes, is
/// damaged.
fn preview_file(assets_path: Option<&str>, converted_dir: &Path) -> PreviewFile {
    let Some(path) = assets_path.and_then(|rel| crate::asset_store::join_under(converted_dir, rel))
    else {
        return PreviewFile::Missing;
    };
    if !path.is_file() {
        return PreviewFile::Missing;
    }
    let fingerprint = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(crate::asset_store::fingerprint_of);
    let intact = fingerprint.is_some_and(|fingerprint| {
        crate::assets_api::hash_file(&path).is_ok_and(|actual| actual == fingerprint)
    });
    if intact {
        PreviewFile::Intact
    } else {
        PreviewFile::Damaged
    }
}

/// Content-addressed relative path: `<aa>/<sha><ext>`.
pub fn derived_rel_path(sha256: &crate::assets_api::Sha256, ext: &str) -> String {
    crate::assets_api::shard_rel_path(sha256, ext)
}

/// MIME type for a derived-media extension, from the shared table in
/// [`media`]; `application/octet-stream` for anything unrecognized.
fn mime_for_ext(ext: &str) -> &'static str {
    media::mime_for_ext(ext).unwrap_or("application/octet-stream")
}

/// Write derived bytes into the content-addressed store; the same bytes always land at the same path.
///
/// A file already at that path is kept only when its bytes hash to the
/// fingerprint in its name. Anything else, such as a version cut short by an
/// interrupted run, is replaced. The bytes go to a synced temporary file in
/// the same directory first and are renamed over the path, so a run killed
/// partway never leaves a partial file under a content-addressed name.
fn store_derived_bytes(derived_dir: &Path, buf: &[u8], ext: &str) -> Result<VersionFile> {
    let sha = crate::assets_api::Sha256::of_bytes(buf);
    let rel = derived_rel_path(&sha, ext);
    let dest = derived_dir.join(&rel);
    let parent = dest
        .parent()
        .with_context(|| format!("derived path {} has no directory", dest.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    if !crate::assets_api::hash_file(&dest).is_ok_and(|actual| actual == sha.as_str()) {
        let mut temporary = crate::asset_store::shard_temp_file(parent)
            .with_context(|| format!("create temporary file in {}", parent.display()))?;
        temporary
            .write_all(buf)
            .with_context(|| format!("write temporary file in {}", parent.display()))?;
        temporary
            .as_file()
            .sync_all()
            .with_context(|| format!("sync temporary file in {}", parent.display()))?;
        temporary
            .persist(&dest)
            .map_err(|err| err.error)
            .with_context(|| format!("install {}", dest.display()))?;
    } else {
        // The file is reused, so it gets a fresh modified time: the sweep at
        // an Import Run's end leaves a young unnamed version alone until
        // `attachment_versions::record` names it.
        fs::File::options()
            .write(true)
            .open(&dest)
            .and_then(|file| file.set_modified(std::time::SystemTime::now()))
            .with_context(|| format!("touch {}", dest.display()))?;
    }
    Ok(VersionFile {
        sha256: sha.to_string(),
        assets_path: rel,
        mime_type: mime_for_ext(ext).to_string(),
    })
}

/// Read a derived file from `work_dir` and store it like [`store_derived_bytes`].
fn store_derived_file(derived_dir: &Path, file_path: &Path, ext: &str) -> Result<VersionFile> {
    let buf = fs::read(file_path)?;
    store_derived_bytes(derived_dir, &buf, ext)
}

#[cfg(test)]
pub(crate) mod tests;
