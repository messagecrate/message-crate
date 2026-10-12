//! The versions the server makes of an attachment's original, as the
//! `attachments` rows name them: the Preview in `preview_*` and the
//! Thumbnail in `thumbnail_*` (`docs/architecture/media.md`, rule 3). Every
//! row of the account that names the original carries the same versions.
//! The column names live here and nowhere else.

use sqlx::SqliteConnection;

use crate::db::begin_write;

/// One of the two versions the server makes of an original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// A copy every browser shows, in `attachments.preview_*`.
    Preview,
    /// A small picture, in `attachments.thumbnail_*`.
    Thumbnail,
}

impl Version {
    /// Both versions.
    pub const ALL: [Self; 2] = [Self::Preview, Self::Thumbnail];

    /// The columns that name this version: fingerprint, path, media type.
    pub(crate) fn columns(self) -> [&'static str; 3] {
        match self {
            Self::Preview => ["preview_sha256", "preview_assets_path", "preview_mime_type"],
            Self::Thumbnail => [
                "thumbnail_sha256",
                "thumbnail_assets_path",
                "thumbnail_mime_type",
            ],
        }
    }

    /// The columns that say why this version could not be made the last
    /// time the media pass tried, `NULL` once it is made: the reason in
    /// words, then its [`NotMadeKind`]. They are set and cleared together,
    /// which a `CHECK` on the table holds.
    pub(crate) fn not_made_columns(self) -> [&'static str; 2] {
        match self {
            Self::Preview => ["preview_not_made_reason", "preview_not_made_kind"],
            Self::Thumbnail => ["thumbnail_not_made_reason", "thumbnail_not_made_kind"],
        }
    }

    /// Whether `staging_attachments` has this version's columns. Staging
    /// copies the Preview columns from the import, and never names a
    /// Thumbnail.
    pub(crate) fn in_staging(self) -> bool {
        self == Self::Preview
    }

    /// This version of the original `sha256`, as a log line names it, such
    /// as `Preview of <sha256>`.
    pub(crate) fn describe_of(self, sha256: &str) -> String {
        format!("{self} of {sha256}")
    }
}

/// The kind of failure that kept the server's media pass from making a
/// Thumbnail or Preview. The reason beside it says the rest in words. The
/// owner's view of an Import Run counts its attachments' failures by kind.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    utoipa::ToSchema,
    sqlx::Type,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum NotMadeKind {
    /// ffmpeg exited with an error: it could not read or convert the file.
    ConversionFailed,
    /// ffmpeg made no progress for `[server] transcode_stall_secs`, so it was
    /// stopped.
    Stalled,
    /// ffmpeg ran past twice the file's length, as it does looping on a
    /// damaged file, so it was stopped.
    RanPastLength,
    /// A step that reads one frame did not finish within its limit.
    TookTooLong,
    /// The original file is not on disk.
    OriginalMissing,
    /// Anything else went wrong on the server.
    ServerProblem,
}

impl NotMadeKind {
    /// Every kind, in the order the owner's view lists them.
    pub const ALL: [Self; 6] = [
        Self::ConversionFailed,
        Self::Stalled,
        Self::RanPastLength,
        Self::TookTooLong,
        Self::OriginalMissing,
        Self::ServerProblem,
    ];
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Preview => "Preview",
            Self::Thumbnail => "Thumbnail",
        })
    }
}

/// A version's file as the rows name it: fingerprint, path under the
/// account's converted directory, and media type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionFile {
    pub sha256: String,
    pub assets_path: String,
    pub mime_type: String,
}

/// One stored original of an account, from every row that names it, with
/// what those rows say about its versions and the names that could hint at
/// its media type.
#[derive(Debug, sqlx::FromRow)]
pub struct StoredOriginal {
    pub sha256: String,
    pub assets_path: String,
    pub mime_type: Option<String>,
    pub preview_assets_path: Option<String>,
    pub preview_sha256: Option<String>,
    pub preview_mime_type: Option<String>,
    /// Rows that name no Preview yet, such as the rows of a source imported
    /// after the Preview was made.
    pub rows_without_preview: i64,
    pub thumbnail_assets_path: Option<String>,
    pub thumbnail_sha256: Option<String>,
    pub thumbnail_mime_type: Option<String>,
    /// Rows that name no Thumbnail yet.
    pub rows_without_thumbnail: i64,
    /// Attachment file name from the export (`attachments.original_name`).
    pub original_name: Option<String>,
    /// Attachment path inside the export (`attachments.path`).
    pub source_path: Option<String>,
    /// What the media pass last decided about whether every browser shows
    /// the original as it is (`attachments.shown_as_is`), `None` until it
    /// has decided. An original that cannot be read is planned from it.
    pub shown_as_is: Option<bool>,
}

impl StoredOriginal {
    /// Extension sources to fall back on when the stored file has no extension.
    pub(crate) fn name_hints(&self) -> [Option<&str>; 2] {
        [self.original_name.as_deref(), self.source_path.as_deref()]
    }

    /// What the rows say about `version`.
    pub(crate) fn named(&self, version: Version) -> Named {
        let (sha256, assets_path, mime_type, rows_without) = match version {
            Version::Preview => (
                &self.preview_sha256,
                &self.preview_assets_path,
                &self.preview_mime_type,
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

/// What the rows of one original say about one of its versions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Named {
    pub sha256: Option<String>,
    pub assets_path: Option<String>,
    pub mime_type: Option<String>,
    /// Rows of the original that name no such version yet, such as the rows
    /// of a source imported after it was made.
    pub rows_without: i64,
}

/// One row per stored original of `account_id`, from every source: every
/// original, or only the one `only` names.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn stored_originals(
    conn: &mut SqliteConnection,
    account_id: i64,
    only: Option<&str>,
) -> Result<Vec<StoredOriginal>, sqlx::Error> {
    // Several messages, from one source or several, can share an original
    // under different names, and only one version of each kind per original
    // is ever made, so collapse those rows and keep any name that could
    // identify the media type.
    sqlx::query_as::<_, StoredOriginal>(
        r"
        SELECT
            a.sha256 AS sha256,
            a.assets_path AS assets_path,
            MAX(a.mime_type) AS mime_type,
            MAX(a.preview_assets_path) AS preview_assets_path,
            MAX(a.preview_sha256) AS preview_sha256,
            MAX(a.preview_mime_type) AS preview_mime_type,
            SUM(CASE WHEN COALESCE(a.preview_assets_path, '') = '' THEN 1 ELSE 0 END)
                AS rows_without_preview,
            MAX(a.thumbnail_assets_path) AS thumbnail_assets_path,
            MAX(a.thumbnail_sha256) AS thumbnail_sha256,
            MAX(a.thumbnail_mime_type) AS thumbnail_mime_type,
            SUM(CASE WHEN COALESCE(a.thumbnail_assets_path, '') = '' THEN 1 ELSE 0 END)
                AS rows_without_thumbnail,
            MAX(a.original_name) AS original_name,
            MAX(a.path) AS source_path,
            MAX(a.shown_as_is) AS shown_as_is
        FROM attachments a
        JOIN messages m ON m.id = a.message_id
        WHERE m.account_id = $1
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
    .await
}

/// The attachment rows of `account_id` for the original `original_sha`.
/// Every write to these rows takes them as one value: a version write
/// ([`VersionWrite`]) and the record of whether the original is shown as it
/// is ([`record_shown_as_is`]). So does the read of where a version is
/// stored ([`file_of`]).
#[derive(Debug, Clone, Copy)]
pub struct OriginalRows<'a> {
    pub account_id: i64,
    pub original_sha: &'a str,
}

/// The rows a version write changes and the file it points them at: the
/// `rows` of an original, pointed at `file` as their `version`. [`record`]
/// and [`share`] take it, so a change to what identifies a version write is
/// made here and where it is built.
#[derive(Debug, Clone, Copy)]
pub struct VersionWrite<'a> {
    pub rows: OriginalRows<'a>,
    pub version: Version,
    pub file: &'a VersionFile,
}

/// Point every attachment row `write` names at its file, and answer how
/// many rows now name it. 0 when every row of the original was deleted
/// meanwhile. The update runs in a write transaction of its own, as every
/// write to `attachments` does (`crate::db::write_tx`).
///
/// # Errors
///
/// Returns a database error when the statement fails.
pub async fn record(
    conn: &mut SqliteConnection,
    write: VersionWrite<'_>,
) -> Result<u64, sqlx::Error> {
    point_rows(conn, write, false).await
}

/// Record on every one of `rows` whether every browser shows the original
/// as it is (`attachments.shown_as_is`), the decision the `/v1` Attachment
/// answers. An original shown as it is needs no Preview, so why one could
/// not be made is cleared with it. Rows that already say so are left alone.
/// The update runs in a write transaction of its own, as [`record`] does.
///
/// # Errors
///
/// Returns a database error when the statement fails.
pub async fn record_shown_as_is(
    conn: &mut SqliteConnection,
    rows: OriginalRows<'_>,
    shown_as_is: bool,
) -> Result<(), sqlx::Error> {
    let OriginalRows {
        account_id,
        original_sha,
    } = rows;
    let [reason_column, kind_column] = Version::Preview.not_made_columns();
    let mut tx = begin_write(conn).await?;
    sqlx::query(&format!(
        "UPDATE attachments
         SET shown_as_is = $1,
             {reason_column} = CASE WHEN $1 THEN NULL ELSE {reason_column} END,
             {kind_column} = CASE WHEN $1 THEN NULL ELSE {kind_column} END
         WHERE sha256 = $2
           AND (shown_as_is IS NOT $1 OR ($1 AND {reason_column} IS NOT NULL))
           AND message_id IN (SELECT id FROM messages WHERE account_id = $3)"
    ))
    .bind(shown_as_is)
    .bind(original_sha)
    .bind(account_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Why a version of an original could not be made, and which rows get it:
/// the `rows` of the original that name no `version`, or name `named`, the
/// file the pass saw as missing or damaged.
#[derive(Debug, Clone, Copy)]
pub struct NotMade<'a> {
    pub rows: OriginalRows<'a>,
    pub version: Version,
    pub named: Option<&'a str>,
    pub kind: NotMadeKind,
    pub reason: &'a str,
}

/// Record why a version could not be made on the rows `not_made` names, the
/// reason the `/v1` Attachment answers. A row that names another file keeps
/// it and no reason: a concurrent pass made that version after this one
/// read the rows. A version made later clears it ([`record`], [`share`]),
/// and so does deciding the original is shown as it is
/// ([`record_shown_as_is`]). The update runs in a write transaction of its
/// own, as [`record`] does.
///
/// # Errors
///
/// Returns a database error when the statement fails.
pub async fn record_not_made(
    conn: &mut SqliteConnection,
    not_made: NotMade<'_>,
) -> Result<(), sqlx::Error> {
    let NotMade {
        rows: OriginalRows {
            account_id,
            original_sha,
        },
        version,
        named,
        kind,
        reason,
    } = not_made;
    let [_, path_column, _] = version.columns();
    let [reason_column, kind_column] = version.not_made_columns();
    let mut tx = begin_write(conn).await?;
    sqlx::query(&format!(
        "UPDATE attachments
         SET {reason_column} = $1, {kind_column} = $5
         WHERE sha256 = $2
           AND (COALESCE({path_column}, '') = '' OR {path_column} = $4)
           AND ({reason_column} IS NOT $1 OR {kind_column} IS NOT $5)
           AND message_id IN (SELECT id FROM messages WHERE account_id = $3)"
    ))
    .bind(reason)
    .bind(original_sha)
    .bind(account_id)
    .bind(named)
    .bind(kind)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Point the attachment rows `write` names that name no version of its
/// kind at its file, and answer how many rows it pointed. Rows that already
/// name one are left alone. 0 when no such row is left, such as when they
/// were deleted meanwhile. The update runs in a write transaction of its
/// own, as [`record`] does.
///
/// # Errors
///
/// Returns a database error when the statement fails.
pub async fn share(
    conn: &mut SqliteConnection,
    write: VersionWrite<'_>,
) -> Result<u64, sqlx::Error> {
    point_rows(conn, write, true).await
}

/// Point the attachment rows `write` names at its file, clear why the
/// version could not be made on them, and answer how many rows it pointed.
/// When `only_unset` is true, rows that already name a version of its kind
/// are left alone. [`record`] and [`share`] are this one statement, so a
/// change to how a version is pointed at is made once.
async fn point_rows(
    conn: &mut SqliteConnection,
    write: VersionWrite<'_>,
    only_unset: bool,
) -> Result<u64, sqlx::Error> {
    let VersionWrite {
        rows: OriginalRows {
            account_id,
            original_sha,
        },
        version,
        file,
    } = write;
    let [sha_column, path_column, mime_column] = version.columns();
    let [reason_column, kind_column] = version.not_made_columns();
    let unset_filter = if only_unset {
        format!("AND COALESCE({path_column}, '') = ''")
    } else {
        String::new()
    };
    let mut tx = begin_write(conn).await?;
    let done = sqlx::query(&format!(
        "UPDATE attachments
         SET {sha_column} = $1, {path_column} = $2, {mime_column} = $3,
             {reason_column} = NULL, {kind_column} = NULL
         WHERE sha256 = $4
           {unset_filter}
           AND message_id IN (SELECT id FROM messages WHERE account_id = $5)"
    ))
    .bind(&file.sha256)
    .bind(&file.assets_path)
    .bind(&file.mime_type)
    .bind(original_sha)
    .bind(account_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(done.rows_affected())
}

/// Clear the `version` columns of every attachment row of `account_id` that
/// names the file at `assets_path`, in a write transaction of its own, as
/// [`record`] does.
///
/// # Errors
///
/// Returns a database error when the statement fails.
pub async fn clear(
    conn: &mut SqliteConnection,
    version: Version,
    account_id: i64,
    assets_path: &str,
) -> Result<(), sqlx::Error> {
    let [sha_column, path_column, mime_column] = version.columns();
    let mut tx = begin_write(conn).await?;
    sqlx::query(&format!(
        "UPDATE attachments
         SET {sha_column} = NULL, {path_column} = NULL, {mime_column} = NULL
         WHERE {path_column} = $1
           AND message_id IN (SELECT id FROM messages WHERE account_id = $2)"
    ))
    .bind(assets_path)
    .bind(account_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Where the `version` of the original `rows` names is stored, and its
/// media type: the path under the account's converted directory. `None`
/// when the account holds no attachment with that fingerprint, or none of
/// its rows names such a version yet.
///
/// The rows should all name one file. When they name more than one, the
/// file the oldest row names answers, with that row's media type, and a
/// warning naming two of the files goes to the server's log.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn file_of(
    conn: &mut SqliteConnection,
    version: Version,
    rows: OriginalRows<'_>,
) -> Result<Option<(String, Option<String>)>, sqlx::Error> {
    let OriginalRows {
        account_id,
        original_sha,
    } = rows;
    let [_, path_column, mime_column] = version.columns();
    // The media pass points every row that names the fingerprint at one
    // version, so they agree. Rows that do not are a damaged store: the
    // oldest row's version answers, so the answer does not change from one
    // read to the next, and the disagreement is logged (#2806). One row per
    // file: with `MIN(a.id)` the only aggregate, SQLite takes the bare media
    // type column from the row that holds the minimum, so each file comes
    // with its oldest row's media type, and rows that differ only in media
    // type are not a disagreement about the file.
    let mut named = sqlx::query_as::<_, (String, Option<String>, i64)>(&format!(
        "SELECT a.{path_column}, a.{mime_column}, MIN(a.id)
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         WHERE m.account_id = $1 AND a.sha256 = $2
           AND a.{path_column} IS NOT NULL AND a.{path_column} != ''
         GROUP BY a.{path_column}
         ORDER BY MIN(a.id)
         LIMIT 2"
    ))
    .bind(account_id)
    .bind(original_sha)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|(path, mime, _oldest)| (path, mime));
    let first = named.next();
    if let (Some((first_path, _)), Some((other_path, _))) = (&first, named.next()) {
        tracing::warn!(
            account_id,
            sha256 = original_sha,
            first_path,
            other_path,
            "The attachment rows of one original name different {version}s, so the oldest row's is served"
        );
    }
    Ok(first)
}

/// True when any attachment of `account_id`, promoted or in staging, names
/// `sha256` as the fingerprint of a file in the converted directory: as its
/// Preview or as its Thumbnail. The two share the directory, so a file
/// there is unused only when no row names it as either.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn converted_file_is_named(
    conn: &mut SqliteConnection,
    account_id: i64,
    sha256: &str,
) -> Result<bool, sqlx::Error> {
    let any_of = |table: &str, staged: bool| {
        Version::ALL
            .iter()
            .filter(|version| !staged || version.in_staging())
            .map(|version| format!("{table}.{} = $2", version.columns()[0]))
            .collect::<Vec<_>>()
            .join(" OR ")
    };
    let promoted = any_of("a", false);
    let staged = any_of("sa", true);
    let found: Option<i64> = sqlx::query_scalar(&format!(
        "SELECT 1 FROM attachments a
         JOIN messages m ON m.id = a.message_id
         WHERE m.account_id = $1 AND ({promoted})
         UNION ALL
         SELECT 1 FROM staging_attachments sa
         JOIN staging_messages sm ON sm.id = sa.message_id
         WHERE sm.account_id = $1 AND ({staged})
         LIMIT 1"
    ))
    .bind(account_id)
    .bind(sha256)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(found.is_some())
}

/// The fingerprints and paths every attachment of `account_id` names,
/// promoted or in staging: of its original and of each version. The sweep
/// keeps a file whose name one of them gives.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn named_files(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Vec<NamedFiles>, sqlx::Error> {
    sqlx::query_as(
        "SELECT a.sha256, a.assets_path, a.preview_sha256, a.preview_assets_path,
                a.thumbnail_sha256, a.thumbnail_assets_path
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         WHERE m.account_id = $1
         UNION ALL
         SELECT sa.sha256, sa.assets_path, sa.preview_sha256, sa.preview_assets_path,
                NULL, NULL
         FROM staging_attachments sa
         JOIN staging_messages sm ON sm.id = sa.message_id
         WHERE sm.account_id = $1",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
}

/// What one attachment row says about the files it names.
#[derive(Debug, sqlx::FromRow)]
pub struct NamedFiles {
    pub sha256: Option<String>,
    pub assets_path: Option<String>,
    pub preview_sha256: Option<String>,
    pub preview_assets_path: Option<String>,
    pub thumbnail_sha256: Option<String>,
    pub thumbnail_assets_path: Option<String>,
}

/// Why one version of an attachment was not made: its kind, and the reason
/// in words, as the `/v1` Attachment answers it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct NotMadeVersion {
    /// The kind of failure.
    pub kind: NotMadeKind,
    /// What ffmpeg said about the file, why it was stopped, or a phrase of
    /// the server's, as the `/v1` Attachment's `preview_not_made_reason` and
    /// `thumbnail_not_made_reason` answer it.
    pub reason: String,
}

/// One attachment an Import Run wrote that has no Thumbnail or no Preview,
/// because the media pass could not make it: the file's name, type and size,
/// which the owner may read (`docs/adr/0008-the-owner-holds-no-messages.md`),
/// and why each version was not made. Never its message, its conversation,
/// or its bytes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct NotMadeAttachment {
    /// Attachment id.
    pub id: i64,
    /// File name from the export.
    pub original_name: Option<String>,
    /// MIME type of the original, when known.
    pub mime_type: Option<String>,
    /// The original's size in bytes, when known.
    pub size_bytes: Option<i64>,
    /// Why the Thumbnail was not made. Null when it was made, when the
    /// attachment gets none, or until the media pass has tried.
    pub thumbnail: Option<NotMadeVersion>,
    /// Why the Preview was not made. Null when it was made, when the
    /// attachment needs none, or until the media pass has tried.
    pub preview: Option<NotMadeVersion>,
}

/// How many of an Import Run's attachments have no Thumbnail, and how many
/// no Preview, for one kind of failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct NotMadeCount {
    /// The kind of failure.
    pub kind: NotMadeKind,
    /// Attachments whose Thumbnail was not made for this kind.
    pub thumbnails: u64,
    /// Attachments whose Preview was not made for this kind.
    pub previews: u64,
}

/// The attachments Import Run `import_id` last wrote (`attachments.import_id`)
/// that have a version the media pass could not make, by id. Empty until
/// the pass has tried them, and for a run whose versions were all made.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn not_made_for_import(
    conn: &mut SqliteConnection,
    import_id: i64,
) -> Result<Vec<NotMadeAttachment>, sqlx::Error> {
    let [thumbnail_reason, thumbnail_kind] = Version::Thumbnail.not_made_columns();
    let [preview_reason, preview_kind] = Version::Preview.not_made_columns();
    let rows: Vec<NotMadeRow> = sqlx::query_as(&format!(
        "SELECT id, original_name, mime_type, size_bytes,
                {thumbnail_kind} AS thumbnail_kind, {thumbnail_reason} AS thumbnail_reason,
                {preview_kind} AS preview_kind, {preview_reason} AS preview_reason
         FROM attachments
         WHERE import_id = $1
           AND ({thumbnail_kind} IS NOT NULL OR {preview_kind} IS NOT NULL)
         ORDER BY id"
    ))
    .bind(import_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(NotMadeRow::into_attachment).collect())
}

/// How many attachments Import Run `import_id` last wrote
/// (`attachments.import_id`): the set [`not_made_for_import`] reads from, so
/// its failures are out of this. It can differ from the run's
/// `attachment_count`, which counts the attachments of the messages the run
/// added, because a run also writes the stored rows it gives their missing
/// file, and a later run that does the same takes them over.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn written_by_import(
    conn: &mut SqliteConnection,
    import_id: i64,
) -> Result<u64, sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE import_id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await?;
    Ok(count.max(0) as u64)
}

/// How many of `attachments` have no Thumbnail and how many no Preview, for
/// each kind of failure, in [`NotMadeKind::ALL`]'s order, leaving out a kind
/// none of them has.
pub fn count_not_made(attachments: &[NotMadeAttachment]) -> Vec<NotMadeCount> {
    let count = |kind: NotMadeKind, version: fn(&NotMadeAttachment) -> Option<&NotMadeVersion>| {
        attachments
            .iter()
            .filter(|attachment| version(attachment).is_some_and(|v| v.kind == kind))
            .count() as u64
    };
    NotMadeKind::ALL
        .into_iter()
        .map(|kind| NotMadeCount {
            kind,
            thumbnails: count(kind, |a| a.thumbnail.as_ref()),
            previews: count(kind, |a| a.preview.as_ref()),
        })
        .filter(|count| count.thumbnails > 0 || count.previews > 0)
        .collect()
}

/// One row [`not_made_for_import`] reads.
#[derive(sqlx::FromRow)]
struct NotMadeRow {
    id: i64,
    original_name: Option<String>,
    mime_type: Option<String>,
    size_bytes: Option<i64>,
    thumbnail_kind: Option<NotMadeKind>,
    thumbnail_reason: Option<String>,
    preview_kind: Option<NotMadeKind>,
    preview_reason: Option<String>,
}

impl NotMadeRow {
    fn into_attachment(self) -> NotMadeAttachment {
        let version = |kind: Option<NotMadeKind>, reason: Option<String>| {
            kind.map(|kind| NotMadeVersion {
                kind,
                reason: reason.unwrap_or_default(),
            })
        };
        NotMadeAttachment {
            id: self.id,
            original_name: self.original_name,
            mime_type: self.mime_type,
            size_bytes: self.size_bytes,
            thumbnail: version(self.thumbnail_kind, self.thumbnail_reason),
            preview: version(self.preview_kind, self.preview_reason),
        }
    }
}
