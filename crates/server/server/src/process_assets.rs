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

use anyhow::{Context, Result, bail};
use sqlx::SqliteConnection;
use tempfile::TempDir;

use crate::config::Config;
use crate::db::schema;
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

/// One of the two versions the server makes of an original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Version {
    /// A copy every browser shows, in `attachments.derived_*`.
    Preview,
    /// A small picture, in `attachments.thumbnail_*`.
    Thumbnail,
}

impl Version {
    /// The columns that name this version: fingerprint, path, media type.
    fn columns(self) -> [&'static str; 3] {
        match self {
            Self::Preview => ["derived_sha256", "derived_assets_path", "derived_mime_type"],
            Self::Thumbnail => [
                "thumbnail_sha256",
                "thumbnail_assets_path",
                "thumbnail_mime_type",
            ],
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Preview => "Preview",
            Self::Thumbnail => "Thumbnail",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DerivedBlob {
    sha256: String,
    assets_path: String,
    mime_type: String,
}

/// What the rows of one original say about one of its versions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Named {
    sha256: Option<String>,
    assets_path: Option<String>,
    mime_type: Option<String>,
    /// Rows of the original that name no such version yet, such as the rows
    /// of a source imported after it was made.
    rows_without: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct AssetRow {
    sha256: String,
    assets_path: String,
    mime_type: Option<String>,
    derived_assets_path: Option<String>,
    /// The Preview's fingerprint and type, as the rows that name it say.
    derived_sha256: Option<String>,
    derived_mime_type: Option<String>,
    rows_without_preview: i64,
    thumbnail_assets_path: Option<String>,
    thumbnail_sha256: Option<String>,
    thumbnail_mime_type: Option<String>,
    rows_without_thumbnail: i64,
    /// Attachment file name from the export (`attachments.original_name`).
    original_name: Option<String>,
    /// Attachment path inside the export (`attachments.path`).
    source_path: Option<String>,
}

impl AssetRow {
    /// Extension sources to fall back on when the stored blob has none.
    fn name_hints(&self) -> [Option<&str>; 2] {
        [self.original_name.as_deref(), self.source_path.as_deref()]
    }

    /// The original's media type, from everything known about it.
    fn media_type(&self) -> Option<String> {
        media::media_type_of(
            Path::new(&self.assets_path),
            self.mime_type.as_deref(),
            &self.name_hints(),
        )
    }

    /// What the rows say about `version`.
    fn named(&self, version: Version) -> Named {
        let (sha256, assets_path, mime_type, rows_without) = match version {
            Version::Preview => (
                &self.derived_sha256,
                &self.derived_assets_path,
                &self.derived_mime_type,
                self.rows_without_preview,
            ),
            Version::Thumbnail => (
                &self.thumbnail_sha256,
                &self.thumbnail_assets_path,
                &self.thumbnail_mime_type,
                self.rows_without_thumbnail,
            ),
        };
        Named {
            sha256: sha256.clone(),
            assets_path: assets_path.clone(),
            mime_type: mime_type.clone(),
            rows_without,
        }
    }
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
pub async fn run(opened: &OpenDb, opts: &ProcessAssetsOptions) -> Result<ProcessAssetsStats> {
    let cfg = &opened.cfg;
    let mut conn = opened.conn().await?;

    let account_ids = match opts.account {
        Some(account_id) => vec![account_id],
        None => list_account_ids(&mut conn, &cfg.paths.data_dir).await?,
    };
    if account_ids.is_empty() {
        bail!("no accounts found — create an account or run reset-demo first");
    }

    let work = TempDir::new().context("create temp dir for derived media")?;
    let mut stats = ProcessAssetsStats::default();

    for &account_id in &account_ids {
        let Some(pass) = AccountPass::open(cfg, opts, work.path(), account_id)? else {
            continue;
        };
        let rows = list_attachments(&mut conn, account_id, None).await?;
        stats.add(&pass.process_rows(&mut conn, &rows).await);
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
/// or an original no attachment names any more, is nothing to do.
///
/// # Errors
///
/// Returns an error when a query fails or the converted directory cannot be
/// made. A version that cannot be made is counted in `errors` and logged.
pub(crate) async fn process_one_asset(
    cfg: &Config,
    conn: &mut SqliteConnection,
    work_dir: &Path,
    account_id: i64,
    sha256: &str,
) -> Result<ProcessAssetsStats> {
    let opts = ProcessAssetsOptions::default();
    let Some(pass) = AccountPass::new(cfg, &opts, work_dir, account_id, Log::Trace)? else {
        return Ok(ProcessAssetsStats::default());
    };
    let rows = list_attachments(conn, account_id, Some(sha256)).await?;
    Ok(pass.process_rows(conn, &rows).await)
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
    Stored(DerivedBlob),
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
                "  {cleaned_verb} {left} temporary file(s) a killed write left in the shard folders"
            );
        }
        Self::new(cfg, opts, work_dir, account_id, Log::Print)
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
            account_id,
            assets_dir,
            converted_dir,
            log,
        }))
    }

    /// `account/path`: how log lines name an attachment.
    fn label(&self, row: &AssetRow) -> String {
        format!("{}/{}", self.account_id, row.assets_path)
    }

    /// Process each of `rows` and count what happened, logging each failure.
    async fn process_rows(
        &self,
        conn: &mut SqliteConnection,
        rows: &[AssetRow],
    ) -> ProcessAssetsStats {
        let mut stats = ProcessAssetsStats::default();
        for row in rows {
            let outcome = self.process(conn, row).await;
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
    async fn process(&self, conn: &mut SqliteConnection, row: &AssetRow) -> Outcome {
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
            browser_shows: media::browser_shows(&source_path, row.media_type().as_deref()),
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
                Need::Share => self
                    .share_existing(conn, row, version)
                    .await
                    .map(|()| false),
                Need::Drop => self.drop_damaged(conn, row, version).await.map(|()| false),
                Need::NoOriginal { damaged } => {
                    let dropped = if damaged {
                        self.drop_damaged(conn, row, version).await
                    } else {
                        Ok(())
                    };
                    dropped.and(Err(anyhow::anyhow!("missing original")))
                }
                Need::Make => {
                    self.make(conn, row, version, versions.kind, &source_path, damaged)
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
        conn: &mut SqliteConnection,
        row: &AssetRow,
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
        update_version(conn, version, self.account_id, &row.sha256, &blob).await?;
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
        conn: &mut SqliteConnection,
        row: &AssetRow,
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
        let blob = DerivedBlob {
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
        update_version(conn, version, self.account_id, &row.sha256, &blob).await?;
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
        conn: &mut SqliteConnection,
        row: &AssetRow,
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
        clear_version(conn, version, self.account_id, &rel).await?;
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
    fn remove_incomplete(&self, row: &AssetRow, source_path: &Path) -> Result<()> {
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
        row: &AssetRow,
    ) -> Result<Derived> {
        let ext = match version {
            Version::Thumbnail => ".jpg",
            Version::Preview => media::preview_extension(kind),
        };
        let token = row.sha256.get(..12).unwrap_or(&row.sha256);
        let out = self.work_dir.join(format!("{version}-{token}{ext}"));
        let made = match version {
            Version::Thumbnail => media::make_thumbnail(source_path, &out),
            Version::Preview => media::make_preview(source_path, kind, &out),
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
        row: &AssetRow,
    ) -> Result<Derived> {
        if self.opts.dry_run {
            self.log
                .say(format!("[dry-run] {version} {} -> {ext}", self.label(row)));
            let _ = fs::remove_file(out);
            return Ok(Derived::DryRun);
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
        let rows = sqlx::query_scalar::<_, i64>("SELECT id FROM accounts ORDER BY id")
            .fetch_all(&mut *conn)
            .await?;
        ids = rows;
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

/// One row per stored original of this account, from every source, with the
/// names that could hint at its media type: every original, or only the one
/// `only` names.
async fn list_attachments(
    conn: &mut SqliteConnection,
    account_id: i64,
    only: Option<&str>,
) -> Result<Vec<AssetRow>> {
    // One row per stored original. Several messages, from one source or
    // several, can share it under different names, and only one version of
    // each kind per original is ever made, so collapse those rows and keep
    // any name that could identify the media type.
    let rows = sqlx::query_as::<_, AssetRow>(
        r"
        SELECT
            a.sha256 AS sha256,
            a.assets_path AS assets_path,
            MAX(a.mime_type) AS mime_type,
            MAX(a.derived_assets_path) AS derived_assets_path,
            MAX(a.derived_sha256) AS derived_sha256,
            MAX(a.derived_mime_type) AS derived_mime_type,
            SUM(CASE WHEN COALESCE(a.derived_assets_path, '') = '' THEN 1 ELSE 0 END)
                AS rows_without_preview,
            MAX(a.thumbnail_assets_path) AS thumbnail_assets_path,
            MAX(a.thumbnail_sha256) AS thumbnail_sha256,
            MAX(a.thumbnail_mime_type) AS thumbnail_mime_type,
            SUM(CASE WHEN COALESCE(a.thumbnail_assets_path, '') = '' THEN 1 ELSE 0 END)
                AS rows_without_thumbnail,
            MAX(a.original_name) AS original_name,
            MAX(a.path) AS source_path
        FROM attachments a
        JOIN messages m ON m.id = a.message_id
        JOIN conversations c ON c.id = m.conversation_id
        WHERE c.account_id = $1
          AND ($2 IS NULL OR a.sha256 = $2)
          AND a.sha256 IS NOT NULL AND a.sha256 != ''
          AND a.assets_path IS NOT NULL AND a.assets_path != ''
        GROUP BY a.sha256, a.assets_path
        ORDER BY a.sha256
        ",
    )
    .bind(account_id)
    .bind(only)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows)
}

/// Point every attachment row of the account for `original_sha`, from every
/// source, at its new `version`.
async fn update_version(
    conn: &mut SqliteConnection,
    version: Version,
    account_id: i64,
    original_sha: &str,
    blob: &DerivedBlob,
) -> Result<()> {
    let [sha_column, path_column, mime_column] = version.columns();
    sqlx::query(&format!(
        r"
        UPDATE attachments
        SET {sha_column} = $1, {path_column} = $2, {mime_column} = $3
        WHERE sha256 = $4
          AND message_id IN (
            SELECT m.id FROM messages m
            JOIN conversations c ON c.id = m.conversation_id
            WHERE c.account_id = $5
          )
        "
    ))
    .bind(&blob.sha256)
    .bind(&blob.assets_path)
    .bind(&blob.mime_type)
    .bind(original_sha)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Clear the `version` columns of every attachment row of the account that
/// names the file at `assets_path`, from every source.
async fn clear_version(
    conn: &mut SqliteConnection,
    version: Version,
    account_id: i64,
    assets_path: &str,
) -> Result<()> {
    let [sha_column, path_column, mime_column] = version.columns();
    sqlx::query(&format!(
        r"
        UPDATE attachments
        SET {sha_column} = NULL, {path_column} = NULL, {mime_column} = NULL
        WHERE {path_column} = $1
          AND message_id IN (
            SELECT m.id FROM messages m
            JOIN conversations c ON c.id = m.conversation_id
            WHERE c.account_id = $2
          )
        "
    ))
    .bind(assets_path)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
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
fn plan(row: &AssetRow, opts: &ProcessAssetsOptions, on_disk: OnDisk) -> Plan {
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
    let is_gif = row.media_type().as_deref() == Some("image/gif");
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
fn store_derived_bytes(derived_dir: &Path, buf: &[u8], ext: &str) -> Result<DerivedBlob> {
    let sha = crate::assets_api::Sha256::of_bytes(buf);
    let rel = derived_rel_path(&sha, ext);
    let dest = derived_dir.join(&rel);
    let parent = dest
        .parent()
        .with_context(|| format!("derived path {} has no folder", dest.display()))?;
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
        // `update_version` names it.
        fs::File::options()
            .write(true)
            .open(&dest)
            .and_then(|file| file.set_modified(std::time::SystemTime::now()))
            .with_context(|| format!("touch {}", dest.display()))?;
    }
    Ok(DerivedBlob {
        sha256: sha.to_string(),
        assets_path: rel,
        mime_type: mime_for_ext(ext).to_string(),
    })
}

/// Read a derived file from `work_dir` and store it like [`store_derived_bytes`].
fn store_derived_file(derived_dir: &Path, file_path: &Path, ext: &str) -> Result<DerivedBlob> {
    let buf = fs::read(file_path)?;
    store_derived_bytes(derived_dir, &buf, ext)
}

#[cfg(test)]
pub(crate) mod tests;
