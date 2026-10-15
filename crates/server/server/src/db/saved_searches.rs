//! Per-account saved searches: named queries a user runs again from the sidebar.
//!
//! A saved search collects nothing. It stores a query string verbatim. The
//! query is checked only for what every list refuses alike, its length, its
//! syntax and its limits ([`crate::search::check_for_every_list`]): each list
//! accepts its own subset of the search language's words, so a query legal
//! for one list can be a `422 Unprocessable Entity` on another (see
//! `search`).
//!
//! Rows are addressed by `id` rather than by name, as Contact Groups and
//! Message Tags are: an edit changes the name and the query together, so a
//! name-addressed update would use the changing field as its key.

use anyhow::Context as _;
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use crate::db::begin_write;

/// How a saved search was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedSearchKind {
    /// A person wrote it.
    Manual,
    /// The server created it at the end of an import run.
    Import,
}

impl SavedSearchKind {
    /// Stored spelling of this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Import => "import",
        }
    }
}

/// One row of `saved_searches`.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SavedSearch {
    /// Saved search id, unique across the database.
    pub id: i64,
    /// Display name, unique per account.
    pub name: String,
    /// Query string, run against the conversation list.
    pub query: String,
    /// `manual` or `import`.
    pub kind: String,
}

/// Create / update / delete failures for a saved search.
#[derive(Debug)]
pub enum SavedSearchError {
    BadRequest(String),
    /// The query breaks a rule of the search language that every list shares.
    Query(crate::search::QueryError),
    NotFound,
    /// Another saved search of the account has the name; carries the
    /// sentence naming it.
    NameTaken(String),
    /// An import found no free name for its saved search on the day;
    /// carries the sentence.
    NoFreeName(String),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for SavedSearchError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

type Result<T> = std::result::Result<T, SavedSearchError>;

/// Map one `saved_searches` row by column name.
fn row_to_saved_search(row: &SqliteRow) -> sqlx::Result<SavedSearch> {
    Ok(SavedSearch {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        query: row.try_get("query")?,
        kind: row.try_get("kind")?,
    })
}

/// Trim and check a name under the rules a Contact Group's and a Message
/// Tag's name follow: not blank, and within the length cap.
fn normalize_name(name: &str) -> Result<String> {
    crate::text_caps::required_capped_text(
        "name",
        name,
        crate::text_caps::MAX_COLLECTION_NAME_CHARS,
    )
    .map(str::to_string)
    .map_err(|refusal| SavedSearchError::BadRequest(refusal.to_string()))
}

/// Trim a query. An empty query is rejected, and so is one every list would
/// refuse; a `word:` is not looked up, because lists differ in their words.
fn normalize_query(query: &str) -> Result<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(SavedSearchError::BadRequest("query required".into()));
    }
    crate::search::check_for_every_list(trimmed).map_err(SavedSearchError::Query)?;
    Ok(trimmed.to_string())
}

/// Id of an account's saved search with this name, case-insensitively.
async fn find_id_by_name(
    conn: &mut SqliteConnection,
    account_id: i64,
    name: &str,
) -> Result<Option<i64>> {
    let id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM saved_searches WHERE account_id = $1 AND lower(name) = lower($2)",
    )
    .bind(account_id)
    .bind(name)
    .fetch_optional(&mut *conn)
    .await
    .context("find the account's Saved Search with a name")?;
    Ok(id)
}

/// One account's saved searches, A–Z.
pub async fn list(conn: &mut SqliteConnection, account_id: i64) -> Result<Vec<SavedSearch>> {
    let searches = sqlx::query(
        "SELECT id, name, query, kind FROM saved_searches WHERE account_id = $1 ORDER BY lower(name)",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .and_then(|rows| rows.iter().map(row_to_saved_search).collect())
    .context("read the account's Saved Searches")?;
    Ok(searches)
}

/// One saved search by id, scoped to the account that owns it.
pub async fn get(
    conn: &mut SqliteConnection,
    account_id: i64,
    id: i64,
) -> Result<Option<SavedSearch>> {
    let search = sqlx::query(
        "SELECT id, name, query, kind FROM saved_searches WHERE account_id = $1 AND id = $2",
    )
    .bind(account_id)
    .bind(id)
    .fetch_optional(&mut *conn)
    .await
    .and_then(|row| row.as_ref().map(row_to_saved_search).transpose())
    .context("read a Saved Search")?;
    Ok(search)
}

/// Create a saved search. The name must be free within the account.
pub async fn create(
    conn: &mut SqliteConnection,
    account_id: i64,
    name: &str,
    query: &str,
    kind: SavedSearchKind,
) -> Result<SavedSearch> {
    let name = normalize_name(name)?;
    let query = normalize_query(query)?;
    // One statement checks the name, inserts the row and answers its id, so
    // a Saved Search created under the name meanwhile, in any letter case,
    // makes this a conflict.
    let Some(id) = crate::db::free_name::insert_if_name_free(
        conn,
        "saved_searches",
        account_id,
        &name,
        &[("query", &query), ("kind", kind.as_str())],
    )
    .await
    .context("insert a Saved Search under a free name")?
    else {
        return Err(SavedSearchError::NameTaken(
            "saved search already exists".into(),
        ));
    };
    Ok(SavedSearch {
        id,
        name,
        query,
        kind: kind.as_str().to_string(),
    })
}

/// Replace a saved search's name and query. `kind` is not editable: it records
/// how the row was born.
pub async fn update(
    conn: &mut SqliteConnection,
    account_id: i64,
    id: i64,
    name: &str,
    query: &str,
) -> Result<SavedSearch> {
    let name = normalize_name(name)?;
    let query = normalize_query(query)?;
    // The checks and the update are one write transaction, so the name
    // cannot be taken, nor the row deleted, between them.
    let mut tx = begin_write(conn)
        .await
        .context("begin a write transaction")?;
    let Some(existing) = get(&mut tx, account_id, id).await? else {
        return Err(SavedSearchError::NotFound);
    };
    // A name already used by a *different* row is a conflict; keeping or
    // recasing this row's own name is not.
    if let Some(other) = find_id_by_name(&mut tx, account_id, &name).await?
        && other != id
    {
        return Err(SavedSearchError::NameTaken(
            "saved search already exists".into(),
        ));
    }
    sqlx::query(
        "UPDATE saved_searches SET name = $1, query = $2 WHERE account_id = $3 AND id = $4",
    )
    .bind(&name)
    .bind(&query)
    .bind(account_id)
    .bind(id)
    .execute(&mut *tx)
    .await
    .context("update a Saved Search's name and query")?;
    tx.commit()
        .await
        .context("commit the Saved Search's new name and query")?;
    Ok(SavedSearch {
        id,
        name,
        query,
        kind: existing.kind,
    })
}

/// Delete a saved search.
///
/// This never touches `imports`: an import-created saved search is a
/// shortcut to a run's messages, and the run's own record is permanent.
pub async fn delete(conn: &mut SqliteConnection, account_id: i64, id: i64) -> Result<()> {
    let result = sqlx::query("DELETE FROM saved_searches WHERE account_id = $1 AND id = $2")
        .bind(account_id)
        .bind(id)
        .execute(&mut *conn)
        .await
        .context("delete a Saved Search")?;
    if result.rows_affected() == 0 {
        return Err(SavedSearchError::NotFound);
    }
    Ok(())
}

/// Create the saved search that points at one import run's messages.
///
/// Called when a run finishes having inserted at least one message. A run
/// that failed, was cancelled, or stored nothing gets no saved search — it is
/// still recorded in `imports` either way.
///
/// The name is `Import <source> <date>`, or that with " 2", " 3", … when the
/// account already has a saved search under the name in any letter case. The
/// insert itself claims the name, through
/// [`crate::db::free_name::insert_under_free_name`], which the run's Contact
/// Group goes through too. A saved search a person made under that name is
/// left alone.
pub async fn create_for_import(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
    source: &str,
    date_ymd: &str,
) -> Result<SavedSearch> {
    let base = normalize_name(&format!("Import {source} {date_ymd}"))?;
    let query = format!("import:#{import_id}");
    let kind = SavedSearchKind::Import.as_str();
    let claimed = crate::db::free_name::insert_under_free_name(
        conn,
        "saved_searches",
        account_id,
        &base,
        &[("query", &query), ("kind", kind)],
    )
    .await
    .context("insert an Import Run's Saved Search under a free name")?;
    let Some((id, name)) = claimed else {
        return Err(SavedSearchError::NoFreeName(
            "too many imports named alike on one day".into(),
        ));
    };
    Ok(SavedSearch {
        id,
        name,
        query,
        kind: kind.to_string(),
    })
}

#[cfg(test)]
mod tests;
