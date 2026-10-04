//! The versions the server makes of an attachment's original, as the
//! `attachments` rows name them: the Preview in `derived_*` and the
//! Thumbnail in `thumbnail_*` (`docs/architecture/media.md`, rule 3). Every
//! row of the account that names the original carries the same versions.
//! The column names live here and nowhere else.

use sqlx::SqliteConnection;

use crate::db::begin_write;

/// One of the two versions the server makes of an original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// A copy every browser shows, in `attachments.derived_*`.
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
            Self::Preview => ["derived_sha256", "derived_assets_path", "derived_mime_type"],
            Self::Thumbnail => [
                "thumbnail_sha256",
                "thumbnail_assets_path",
                "thumbnail_mime_type",
            ],
        }
    }

    /// Whether `staging_attachments` has this version's columns. Staging
    /// copies the Preview columns from the import, and never names a
    /// Thumbnail.
    pub(crate) fn in_staging(self) -> bool {
        self == Self::Preview
    }
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
    pub derived_assets_path: Option<String>,
    pub derived_sha256: Option<String>,
    pub derived_mime_type: Option<String>,
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
}

impl StoredOriginal {
    /// Extension sources to fall back on when the stored blob has none.
    pub(crate) fn name_hints(&self) -> [Option<&str>; 2] {
        [self.original_name.as_deref(), self.source_path.as_deref()]
    }

    /// What the rows say about `version`.
    pub(crate) fn named(&self, version: Version) -> Named {
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

/// Point every attachment row of `account_id` for the original
/// `original_sha` at `file` as its `version`, and answer how many rows now
/// name it. 0 when every row of the original was deleted meanwhile. The
/// update runs in a write transaction of its own, as every write to
/// `attachments` does (`crate::db::write_tx`).
///
/// # Errors
///
/// Returns a database error when the statement fails.
pub async fn record(
    conn: &mut SqliteConnection,
    version: Version,
    account_id: i64,
    original_sha: &str,
    file: &VersionFile,
) -> Result<u64, sqlx::Error> {
    let [sha_column, path_column, mime_column] = version.columns();
    let mut tx = begin_write(conn).await?;
    let done = sqlx::query(&format!(
        "UPDATE attachments
         SET {sha_column} = $1, {path_column} = $2, {mime_column} = $3
         WHERE sha256 = $4
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

/// Where the `version` of one of the account's originals is stored, and its
/// media type: the path under the account's converted directory. `None`
/// when the account holds no attachment with that fingerprint, or none of
/// its rows names such a version yet.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn file_of(
    conn: &mut SqliteConnection,
    version: Version,
    account_id: i64,
    sha256: &str,
) -> Result<Option<(String, Option<String>)>, sqlx::Error> {
    let [_, path_column, mime_column] = version.columns();
    // Every row that names the fingerprint carries the same version, so any
    // one of them answers.
    sqlx::query_as::<_, (String, Option<String>)>(&format!(
        "SELECT a.{path_column}, a.{mime_column}
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         WHERE m.account_id = $1 AND a.sha256 = $2
           AND a.{path_column} IS NOT NULL AND a.{path_column} != ''
         LIMIT 1"
    ))
    .bind(account_id)
    .bind(sha256)
    .fetch_optional(&mut *conn)
    .await
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
        "SELECT a.sha256, a.assets_path, a.derived_sha256, a.derived_assets_path,
                a.thumbnail_sha256, a.thumbnail_assets_path
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         WHERE m.account_id = $1
         UNION ALL
         SELECT sa.sha256, sa.assets_path, sa.derived_sha256, sa.derived_assets_path,
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
    pub derived_sha256: Option<String>,
    pub derived_assets_path: Option<String>,
    pub thumbnail_sha256: Option<String>,
    pub thumbnail_assets_path: Option<String>,
}
