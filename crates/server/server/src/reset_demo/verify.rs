//! The checks a `reset-demo` run makes before it swaps the rebuilt database
//! in: that no row outside the Demo Account changed, that the old Demo
//! Account's Audit Trail outlived it, and that no other account's directory
//! changed (#1225, #1450). The reads themselves are in [`crate::db::demo`].

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sqlx::SqliteConnection;

use crate::db::demo::{
    DemoRows, OutlivingRows, TableDigest, last_audit_entry, non_demo_state_on_conn, outliving_rows,
    sample_search_terms,
};
use crate::db::{engine, schema};

/// Refuse to install the prepared database when it changed anything outside
/// the Demo Account: a row of any other account in any table, or a row that
/// belongs to no account, such as the Server Settings. A reset must only ever
/// touch the Demo Account (#1225). Refuse it too when it lost the old Demo
/// Account's Audit Trail entries or runs, which its deletion only unlinks
/// (ADR 0020).
pub(super) async fn verify_non_demo_state_preserved(
    active: &Path,
    prepared: &Path,
    demo_id: i64,
) -> Result<()> {
    if !active.is_file() {
        return Ok(());
    }
    let active_side = with_check_conn(active, async |conn| {
        if !schema::table_exists(conn, "accounts").await? {
            // A database with no accounts holds nothing a reset could lose.
            return Ok(None);
        }
        let demo = DemoRows {
            account: demo_id,
            last_entry_before_reset: last_audit_entry(conn).await?,
        };
        let search_terms = sample_search_terms(conn, demo_id).await?;
        let state = non_demo_state_on_conn(conn, demo, &search_terms).await?;
        let record = outliving_rows(conn, OutlivingRows::OfAccount(demo_id)).await?;
        Ok(Some((demo, search_terms, state, record)))
    })
    .await
    .with_context(|| format!("read the non-demo rows of {}", active.display()))?;
    let Some((demo, search_terms, active_state, demo_record)) = active_side else {
        return Ok(());
    };
    let (prepared_state, unlinked_record) = with_check_conn(prepared, async |conn| {
        let state = non_demo_state_on_conn(conn, demo, &search_terms).await?;
        let unlinked = outliving_rows(
            conn,
            OutlivingRows::UnlinkedAfter(demo.last_entry_before_reset),
        )
        .await?;
        Ok((state, unlinked))
    })
    .await
    .with_context(|| format!("read the non-demo rows of {}", prepared.display()))?;
    let changed = changed_entries(&active_state, &prepared_state, |table, active, prepared| {
        let rows = |digest: Option<&TableDigest>| {
            digest.map_or_else(|| "no table".to_owned(), |digest| digest.rows.to_string())
        };
        format!(
            "{table} (active rows={}, prepared rows={})",
            rows(active),
            rows(prepared)
        )
    });
    if !changed.is_empty() {
        bail!(
            "prepared reset database changed rows outside the Demo Account in: {}",
            changed.join(", ")
        );
    }
    let lost: Vec<String> = demo_record
        .iter()
        .filter_map(|(table, ids)| {
            let kept = unlinked_record.get(table);
            let missing = ids
                .iter()
                .filter(|id| !kept.is_some_and(|kept| kept.contains(id)))
                .count();
            let rows = if missing == 1 { "row" } else { "rows" };
            (missing > 0).then(|| format!("{table} ({missing} {rows})"))
        })
        .collect();
    if !lost.is_empty() {
        bail!(
            "prepared reset database lost the old Demo Account's Audit Trail in: {}",
            lost.join(", ")
        );
    }
    Ok(())
}

/// The keys whose values differ between `before` and `after`, in order, each
/// described by `describe` with both values.
fn changed_entries<K: Ord, V: PartialEq>(
    before: &BTreeMap<K, V>,
    after: &BTreeMap<K, V>,
    mut describe: impl FnMut(&K, Option<&V>, Option<&V>) -> String,
) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|key| before.get(*key) != after.get(*key))
        .map(|key| describe(key, before.get(key), after.get(key)))
        .collect()
}

/// Run `read` on one connection to the database at `db`, then close it.
pub(super) async fn with_check_conn<T>(
    db: &Path,
    read: impl AsyncFnOnce(&mut SqliteConnection) -> Result<T>,
) -> Result<T> {
    let pool = engine::open_pool_for_path(db)
        .await
        .with_context(|| format!("open {} to verify non-demo accounts", db.display()))?;
    let mut conn = pool
        .acquire()
        .await
        .with_context(|| format!("open {} to verify non-demo accounts", db.display()))?;
    let result = read(&mut conn).await;
    conn.close().await?;
    pool.close().await;
    result
}

/// One entry under an account's directory, as [`other_account_directories`] lists
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DirectoryEntry {
    Directory,
    File {
        bytes: u64,
    },
    /// A symbolic link or anything else that is neither a file nor a directory.
    Other,
}

impl std::fmt::Display for DirectoryEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Directory => f.write_str("directory"),
            Self::File { bytes } => write!(f, "{bytes} bytes"),
            Self::Other => f.write_str("not a file or directory"),
        }
    }
}

/// Every file and directory in the directories of accounts other than `demo_id`
/// under `data_dir` (`data_dir/<account>/`), by its path relative to
/// `data_dir`, with each file's size. A missing `data_dir` lists nothing.
/// An account directory that is a symbolic link is followed, since an account
/// moved to another disk is linked back that way; below it, links are listed
/// and not followed ([`list_directory`]).
///
/// # Errors
///
/// Returns an error when a directory cannot be read, or an account directory is a
/// link to nothing.
pub(super) fn other_account_directories(
    data_dir: &Path,
    demo_id: i64,
) -> Result<BTreeMap<PathBuf, DirectoryEntry>> {
    let mut listing = BTreeMap::new();
    let entries = match fs::read_dir(data_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(listing),
        Err(error) => {
            return Err(error).with_context(|| format!("list {}", data_dir.display()));
        }
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("list {}", data_dir.display()))?;
        let name = entry.file_name();
        let is_other_account = name
            .to_str()
            .and_then(|name| name.parse::<i64>().ok())
            .is_some_and(|account| account != demo_id);
        if !is_other_account {
            continue;
        }
        let is_directory = fs::metadata(entry.path())
            .with_context(|| format!("read the account directory {}", entry.path().display()))?
            .is_dir();
        if is_directory {
            list_directory(data_dir, &entry.path(), &mut listing)?;
        }
    }
    Ok(listing)
}

/// Add `directory` and everything under it to `listing`, by paths relative to
/// `root`. A symbolic link is listed and not followed.
fn list_directory(
    root: &Path,
    directory: &Path,
    listing: &mut BTreeMap<PathBuf, DirectoryEntry>,
) -> Result<()> {
    let relative = |path: &Path| path.strip_prefix(root).unwrap_or(path).to_path_buf();
    listing.insert(relative(directory), DirectoryEntry::Directory);
    for entry in fs::read_dir(directory).with_context(|| format!("list {}", directory.display()))? {
        let entry = entry.with_context(|| format!("list {}", directory.display()))?;
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).with_context(|| format!("read {}", path.display()))?;
        if metadata.is_dir() {
            list_directory(root, &path, listing)?;
        } else if metadata.is_file() {
            listing.insert(
                relative(&path),
                DirectoryEntry::File {
                    bytes: metadata.len(),
                },
            );
        } else {
            listing.insert(relative(&path), DirectoryEntry::Other);
        }
    }
    Ok(())
}

/// The most changed paths a refusal names before it counts the rest.
const CHANGED_PATHS_NAMED: usize = 10;

/// Refuse to install the reset when the directory of any account other than
/// `demo_id` under `data_dir` no longer lists what `before` lists: a file or
/// directory added or removed, or a file whose size changed. The reset only
/// ever replaces the Demo Account's directory (#1450).
pub(super) fn verify_other_account_directories_preserved(
    data_dir: &Path,
    demo_id: i64,
    before: &BTreeMap<PathBuf, DirectoryEntry>,
) -> Result<()> {
    let after = other_account_directories(data_dir, demo_id)?;
    let changed = changed_entries(before, &after, |path, before, after| {
        let side = |entry: Option<&DirectoryEntry>| {
            entry.map_or_else(|| "none".to_owned(), ToString::to_string)
        };
        format!(
            "{} (before: {}, after: {})",
            path.display(),
            side(before),
            side(after)
        )
    });
    if changed.is_empty() {
        return Ok(());
    }
    let mut named = changed[..changed.len().min(CHANGED_PATHS_NAMED)].join(", ");
    if changed.len() > CHANGED_PATHS_NAMED {
        named.push_str(&format!(
            " and {} more",
            changed.len() - CHANGED_PATHS_NAMED
        ));
    }
    bail!("the reset changed other accounts' directories: {named}");
}
