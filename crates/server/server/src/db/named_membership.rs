//! Shared storage for named sets (Message Tags and Contact Groups).
//!
//! Both domains store a named set (rows in a names table) whose members are
//! conversation or contact ids. The HTTP layer addresses a set by id through
//! `list_sets`, `get_set`, `create_set`, `rename_set`, `delete_set`,
//! `list_member_ids_of`, and `patch_members`. Only tests fill a set by name,
//! through `set_membership`, which creates the name on demand.
//! The operations are identical apart from table and column names, reserved
//! names, and one post-change hook, so this module implements them once
//! behind [`MembershipSpec`].

use std::future::Future;
use std::pin::Pin;

use anyhow::{Context as _, Result as AnyResult};
use chrono::{DateTime, Utc};
use sqlx::{AssertSqlSafe, SqlSafeStr, SqlStr, SqliteConnection};

use crate::db::begin_write;
use crate::text_caps::{MAX_COLLECTION_NAME_CHARS, required_capped_text};

/// One Contact Group or Message Tag: its id and name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct NamedSet {
    /// Id of the Contact Group or Message Tag, which its routes name in their
    /// path.
    pub id: i64,
    /// The name as stored: trimmed, at most 80 characters, and unique among
    /// the account's sets of this kind whatever its letter case.
    pub name: String,
}

/// Create / rename / delete / membership failures for a named set.
#[derive(Debug)]
pub enum MembershipError {
    BadRequest(String),
    /// A member id this account does not hold; carries the sentence naming it.
    NotFound(String),
    /// The set itself is missing; carries its label (`"tag"` / `"group"`).
    SetNotFound(&'static str),
    /// Another set of the account has the name; carries the sentence naming
    /// it.
    NameTaken(String),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for MembershipError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

/// Extra work after a membership change, async over the connection borrow.
type ChangeHook = for<'a> fn(
    &'a mut SqliteConnection,
    i64,
    i64,
    DateTime<Utc>,
) -> Pin<Box<dyn Future<Output = AnyResult<()>> + Send + 'a>>;

/// Table names, labels, reserved names, and messages for one named set.
///
/// `name_column` and `member_column` live on the membership table;
/// `member_table` is the table members must exist in. All values are compile
/// time constants, so the SQL built from them is fixed at build time.
pub struct MembershipSpec {
    /// Names table (`message_tags` / `contact_groups`).
    pub table: &'static str,
    /// Membership table (`message_tag_members` / `contact_group_members`).
    pub members_table: &'static str,
    /// Column on the membership table that references the names table.
    pub name_column: &'static str,
    /// Column on the membership table that holds the member id.
    pub member_column: &'static str,
    /// Table members must exist in (`conversations` / `contacts`).
    pub member_table: &'static str,
    /// Singular label used in error messages (`"tag"` / `"group"`).
    pub label: &'static str,
    /// What a reserved-name message calls the set (`"tag"` / `"Contact Group"`).
    pub reserved_label: &'static str,
    /// Member label used in error messages (`"conversation"` / `"contact"`).
    pub member_label: &'static str,
    /// Names that must not be created.
    pub reserved: &'static [&'static str],
    /// Reserved names with dedicated error messages (lowercase name, message).
    pub special_reserved: &'static [(&'static str, &'static str)],
    /// A character a name may not hold, with the reason written to follow
    /// "because".
    pub refused_char: Option<(char, &'static str)>,
    /// Extra work after a membership change, at the instant the change passes
    /// (groups touch the contact row).
    pub on_change: Option<ChangeHook>,
}

/// Message tags on conversations.
pub fn tag_spec() -> &'static MembershipSpec {
    static SPEC: MembershipSpec = MembershipSpec {
        table: "message_tags",
        members_table: "message_tag_members",
        name_column: "tag_id",
        member_column: "conversation_id",
        member_table: "conversations",
        label: "tag",
        reserved_label: "Message Tag",
        member_label: "conversation",
        reserved: &[
            "home",
            "contacts",
            "threads",
            "thread",
            "all",
            "excluded",
            "unassigned",
            "trash",
            "tags",
            "tag",
            "no-tag",
            "no tag",
            "groups",
            "group",
            "labels",
            "label",
            // `tag:none` means "no tag", quoted or not, so a tag under this
            // name would list the untagged conversations.
            "none",
        ],
        special_reserved: &[],
        // Message Tags are not in the address book, so a tag may hold `;`.
        refused_char: None,
        on_change: None,
    };
    &SPEC
}

/// Contact groups on contacts.
pub fn group_spec() -> &'static MembershipSpec {
    static SPEC: MembershipSpec = MembershipSpec {
        table: "contact_groups",
        members_table: "contact_group_members",
        name_column: "group_id",
        member_column: "contact_id",
        member_table: "contacts",
        label: "group",
        reserved_label: "Contact Group",
        member_label: "contact",
        reserved: &[
            "home",
            "contacts",
            "all",
            "excluded",
            "no-messages",
            "no messages",
            "unassigned",
            "trash",
            "groups",
            "group",
            "group-chats",
            "group chats",
            "group-chats-2",
            "group chats 2",
            "group-messages",
            "group messages",
            "group-messages-2",
            "group messages 2",
            "no-label",
            "no-group",
            "no group",
            "labels",
            "label",
            "no label",
            // The two computed groups, as `group:` names them.
            "unknown",
            "none",
        ],
        special_reserved: &[
            ("contacts", "Contacts is a reserved Contact Group"),
            ("all", "All is a reserved Contact Group"),
            ("excluded", "Excluded is a reserved Contact Group"),
            ("unassigned", "Unassigned is a reserved Contact Group"),
            ("trash", "Trash is a reserved Contact Group"),
            ("no messages", "No messages is a reserved Contact Group"),
            ("no-messages", "No messages is a reserved Contact Group"),
        ],
        // A name holding `;` would come back from an address book export
        // and load as two Contact Groups.
        refused_char: Some((
            crate::db::address_book::GROUP_SEPARATOR,
            "the address book separates Contact Group names with it",
        )),
        on_change: Some(touch_member_owner),
    };
    &SPEC
}

/// Set the member contact's updated-at to `now`, boxed so the spec table can
/// hold it as a plain function pointer.
fn touch_member_owner<'a>(
    conn: &'a mut SqliteConnection,
    account_id: i64,
    member_id: i64,
    now: DateTime<Utc>,
) -> Pin<Box<dyn Future<Output = AnyResult<()>> + Send + 'a>> {
    Box::pin(crate::db::contacts::touch_contact(
        conn, account_id, member_id, now,
    ))
}

/// Id of the named set called `name`, if it exists.
async fn find_id(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    name: &str,
) -> Result<Option<i64>, MembershipError> {
    let sql = format!(
        "SELECT id FROM {table} WHERE account_id = $1 AND lower(name) = lower($2)",
        table = spec.table,
    );
    let id = sqlx::query_scalar::<_, i64>(AssertSqlSafe(sql))
        .bind(account_id)
        .bind(name)
        .fetch_optional(&mut *conn)
        .await
        .context("find a named set by its name")?;
    Ok(id)
}

/// Id of the named set called `name`, creating it if needed.
#[cfg(test)]
async fn ensure_id(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    name: &str,
) -> Result<i64, MembershipError> {
    let name = normalize_name(spec, name)?;
    let sql = format!(
        "INSERT INTO {table} (account_id, name) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        table = spec.table
    );
    sqlx::query(AssertSqlSafe(sql))
        .bind(account_id)
        .bind(&name)
        .execute(&mut *conn)
        .await
        .context("insert a named set")?;
    find_id(spec, conn, account_id, &name)
        .await?
        .ok_or_else(|| {
            MembershipError::Internal(anyhow::anyhow!("failed to ensure {} {name}", spec.label))
        })
}

/// True when `name` is reserved and must not be created.
pub fn is_reserved(spec: &MembershipSpec, name: &str) -> bool {
    let key = name.trim().to_ascii_lowercase();
    spec.reserved.contains(&key.as_str())
}

/// The message for a reserved name: the spec's specific one, or the generic one.
fn reserved_error(spec: &MembershipSpec, name: &str) -> String {
    let key = name.trim().to_ascii_lowercase();
    for (reserved, message) in spec.special_reserved {
        if key == *reserved {
            return (*message).to_string();
        }
    }
    format!("\"{}\" is a reserved {}", name.trim(), spec.reserved_label)
}

/// Trim and validate a set name: not blank and within the cap every
/// collection name shares, then the spec's refused-character and
/// reserved-name rules.
fn normalize_name(spec: &MembershipSpec, name: &str) -> Result<String, MembershipError> {
    let trimmed = required_capped_text("name", name, MAX_COLLECTION_NAME_CHARS)
        .map_err(|refusal| MembershipError::BadRequest(refusal.to_string()))?;
    if let Some((refused, reason)) = spec.refused_char
        && trimmed.contains(refused)
    {
        return Err(MembershipError::BadRequest(format!(
            "name can't hold \"{refused}\", because {reason}"
        )));
    }
    if is_reserved(spec, trimmed) {
        return Err(MembershipError::BadRequest(reserved_error(spec, trimmed)));
    }
    Ok(trimmed.to_string())
}

/// The trimmed name when a person could create a set under it, else the
/// sentence that says why not: blank, too long, holding the refused
/// character, or reserved. For a caller
/// that creates sets by name outside the set routes, as an address book load
/// does for the Contact Groups its rows list.
///
/// # Errors
///
/// The reason the name is refused.
pub fn check_name(spec: &MembershipSpec, name: &str) -> Result<String, String> {
    match normalize_name(spec, name) {
        Ok(name) => Ok(name),
        Err(MembershipError::BadRequest(reason)) => Err(reason),
        Err(_) => Err("name refused".to_string()),
    }
}

/// True when the member row belongs to this account.
async fn member_exists(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    member_id: i64,
) -> Result<bool, MembershipError> {
    let sql = format!(
        "SELECT id FROM {mt} WHERE id = $1 AND account_id = $2",
        mt = spec.member_table
    );
    let found: Option<i64> = sqlx::query_scalar::<_, i64>(AssertSqlSafe(sql))
        .bind(member_id)
        .bind(account_id)
        .fetch_optional(&mut *conn)
        .await
        .context("check that the account holds a member")?;
    Ok(found.is_some())
}

/// Add or remove one name for many members. Creates the name when enabling.
///
/// Test builds only: tests across the server use it to put a contact in a
/// group in one call. Routes go by id through `patch_members`, and an import
/// makes its own group with `db::contacts::create_import_group`.
#[cfg(test)]
pub async fn set_membership(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    member_ids: &[i64],
    name: &str,
    enable: bool,
    now: DateTime<Utc>,
) -> Result<u64, MembershipError> {
    let ids = clean_ids(member_ids);
    if ids.is_empty() {
        return Err(MembershipError::BadRequest(format!(
            "{} ids required",
            spec.member_label
        )));
    }
    let name_trimmed = name.trim();
    if name_trimmed.is_empty() {
        return Err(MembershipError::BadRequest(format!(
            "{} name required",
            spec.label
        )));
    }
    if is_reserved(spec, name_trimmed) {
        return Err(MembershipError::BadRequest(reserved_error(
            spec,
            name_trimmed,
        )));
    }

    for id in &ids {
        if !member_exists(spec, conn, account_id, *id).await? {
            return Err(MembershipError::NotFound(format!(
                "{} {id} not found",
                spec.member_label
            )));
        }
    }

    let name_row_id = if enable {
        ensure_id(spec, conn, account_id, name_trimmed).await?
    } else {
        match find_id(spec, conn, account_id, name_trimmed).await? {
            Some(id) => id,
            None => return Ok(0),
        }
    };

    let mut changed = 0u64;
    for id in ids {
        let n = if enable {
            sqlx::query(insert_member_sql(spec))
                .bind(name_row_id)
                .bind(id)
                .bind(account_id)
                .execute(&mut *conn)
                .await
                .context("add a member to a named set")?
                .rows_affected()
        } else {
            sqlx::query(delete_member_sql(spec))
                .bind(id)
                .bind(name_row_id)
                .bind(account_id)
                .execute(&mut *conn)
                .await
                .context("remove a member from a named set")?
                .rows_affected()
        };
        if n > 0 {
            changed += 1;
            if let Some(hook) = spec.on_change {
                hook(conn, account_id, id, now)
                    .await
                    .map_err(MembershipError::Internal)?;
            }
        }
    }
    Ok(changed)
}

/// Drop non-positive ids, sort, and dedupe a caller's member id list.
fn clean_ids(ids: &[i64]) -> Vec<i64> {
    sorted_ids(ids.iter().copied().filter(|id| *id > 0).collect())
}

/// `ids` sorted, each once.
fn sorted_ids(mut ids: Vec<i64>) -> Vec<i64> {
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Sets for this account, A–Z, excluding reserved leftovers.
pub async fn list_sets(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Vec<NamedSet>, MembershipError> {
    let sql = format!(
        "SELECT id, name FROM {table} WHERE account_id = $1 ORDER BY lower(name)",
        table = spec.table
    );
    let rows = sqlx::query_as::<_, NamedSet>(AssertSqlSafe(sql))
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await
        .context("read the account's named sets")?;
    Ok(rows
        .into_iter()
        .filter(|set| !is_reserved(spec, &set.name))
        .collect())
}

/// One set by id, or `SetNotFound` when it is not this account's.
pub async fn get_set(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    id: i64,
) -> Result<NamedSet, MembershipError> {
    let sql = format!(
        "SELECT id, name FROM {table} WHERE id = $1 AND account_id = $2",
        table = spec.table
    );
    let set = sqlx::query_as::<_, NamedSet>(AssertSqlSafe(sql))
        .bind(id)
        .bind(account_id)
        .fetch_optional(&mut *conn)
        .await
        .context("read a named set")?
        .ok_or(MembershipError::SetNotFound(spec.label))?;
    // A reserved-name row can only be a leftover (create_set and rename_set
    // both refuse reserved names): list_sets never shows it, so its id must
    // not work either.
    if is_reserved(spec, &set.name) {
        return Err(MembershipError::SetNotFound(spec.label));
    }
    Ok(set)
}

/// Create a set and answer it, under its trimmed name. Fails when the name
/// is taken (ignoring case) or reserved.
pub async fn create_set(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    name: &str,
) -> Result<NamedSet, MembershipError> {
    let name = normalize_name(spec, name)?;
    // One statement checks the name and inserts the row, so a set created
    // under the name meanwhile, in any letter case, makes this a conflict.
    match crate::db::free_name::insert_if_name_free(conn, spec.table, account_id, &name, &[])
        .await
        .context("insert a named set under a free name")?
    {
        Some(id) => Ok(NamedSet { id, name }),
        None => Err(MembershipError::NameTaken(format!(
            "{} already exists",
            spec.label
        ))),
    }
}

/// Rename a set by id and answer it under its new name. A case-only change
/// of its own name is allowed; another set's name (ignoring case) is a
/// conflict.
pub async fn rename_set(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    id: i64,
    name: &str,
) -> Result<NamedSet, MembershipError> {
    let new_name = normalize_name(spec, name)?;
    // The checks and the update are one write transaction, so the name
    // cannot be taken, nor the set deleted, between them.
    let mut tx = begin_write(conn)
        .await
        .context("begin a write transaction")?;
    let old = get_set(spec, &mut tx, account_id, id).await?;
    if old.name == new_name {
        return Ok(old);
    }
    if let Some(other) = find_id(spec, &mut tx, account_id, &new_name).await?
        && other != id
    {
        return Err(MembershipError::NameTaken(format!(
            "{} already exists",
            spec.label
        )));
    }
    let sql = format!(
        "UPDATE {table} SET name = $1 WHERE id = $2 AND account_id = $3",
        table = spec.table
    );
    sqlx::query(AssertSqlSafe(sql))
        .bind(&new_name)
        .bind(id)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .context("rename a named set")?;
    tx.commit()
        .await
        .context("commit the named set's new name")?;
    Ok(NamedSet { id, name: new_name })
}

/// Delete a set by id, and its memberships, in one transaction. The
/// `on_change` hook runs once for each member, at `now`, as it does when
/// `patch_members` takes a member out, because each member's sets changed.
pub async fn delete_set(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    id: i64,
    now: DateTime<Utc>,
) -> Result<(), MembershipError> {
    let mut tx = begin_write(conn)
        .await
        .context("begin a write transaction")?;
    get_set(spec, &mut tx, account_id, id).await?;
    let members_sql = format!(
        "DELETE FROM {mt} WHERE {nc} = $1 RETURNING {mc}",
        mt = spec.members_table,
        nc = spec.name_column,
        mc = spec.member_column,
    );
    let members = sqlx::query_scalar::<_, i64>(AssertSqlSafe(members_sql))
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .context("delete the members of a named set")?;
    let sql = format!(
        "DELETE FROM {table} WHERE id = $1 AND account_id = $2",
        table = spec.table
    );
    sqlx::query(AssertSqlSafe(sql))
        .bind(id)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .context("delete a named set")?;
    if let Some(hook) = spec.on_change {
        for member in members {
            hook(&mut tx, account_id, member, now)
                .await
                .map_err(MembershipError::Internal)?;
        }
    }
    tx.commit()
        .await
        .context("commit the named set's deletion")?;
    Ok(())
}

/// Member ids of one set, ascending.
pub async fn list_member_ids_of(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    id: i64,
) -> Result<Vec<i64>, MembershipError> {
    get_set(spec, conn, account_id, id).await?;
    let sql = format!(
        "SELECT {mc} FROM {mt} WHERE {nc} = $1 ORDER BY {mc}",
        mc = spec.member_column,
        mt = spec.members_table,
        nc = spec.name_column,
    );
    let rows = sqlx::query_scalar::<_, i64>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_all(&mut *conn)
        .await
        .context("read the members of a named set")?;
    Ok(rows)
}

/// Add and remove members of one set in one call, answering
/// `(added, removed)`. Every id in `add` is checked before anything is
/// written, so a foreign or unknown id there leaves the set as it was. An id
/// in `remove` that names no member, or a row the caller does not hold, is
/// ignored, because the set is already in the state the caller asked for;
/// `removed` counts only the rows that were deleted. An id present in both
/// `add` and `remove` nets to "removed": it is dropped from `add` so it is
/// deleted, not inserted then deleted, and the `on_change` hook fires once
/// for it rather than twice. The hook runs at `now`.
pub async fn patch_members(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    id: i64,
    add: &[i64],
    remove: &[i64],
    now: DateTime<Utc>,
) -> Result<(u64, u64), MembershipError> {
    // The checks and the writes are one write transaction: the set and every
    // id to add are as they were checked when the rows go in, and a failure
    // part-way leaves the set as it was.
    let mut tx = begin_write(conn)
        .await
        .context("begin a write transaction")?;
    let conn: &mut SqliteConnection = &mut tx;
    get_set(spec, conn, account_id, id).await?;
    let remove = clean_ids(remove);
    // An id to add of 0 or below is kept, unlike one to remove: it names no
    // row, so the check below refuses it as it refuses any other id the
    // caller does not hold, rather than the call dropping it unseen.
    let mut add = sorted_ids(add.to_vec());
    add.retain(|id| !remove.contains(id));
    if add.is_empty() && remove.is_empty() {
        return Err(MembershipError::BadRequest(format!(
            "{} ids required",
            spec.member_label
        )));
    }
    // An id to add that names no row the caller holds is a body that broke a
    // rule, `422 Unprocessable Entity`, not an address that is missing
    // (`docs/architecture/rules/http-api.md`, API-33).
    for member in &add {
        if !member_exists(spec, conn, account_id, *member).await? {
            return Err(MembershipError::BadRequest(format!(
                "{} {member} not found",
                spec.member_label
            )));
        }
    }
    // An id to remove is not checked: `delete_member_sql` deletes only a row
    // the caller holds, and a member that was never in the set is already in
    // the state the caller asked for (`docs/architecture/rules/http-api.md`,
    // API-34).

    let insert_sql = insert_member_sql(spec);
    let delete_sql = delete_member_sql(spec);

    let mut added = 0u64;
    for member in add {
        let n = sqlx::query(insert_sql.clone())
            .bind(id)
            .bind(member)
            .bind(account_id)
            .execute(&mut *conn)
            .await
            .context("add a member to a named set")?
            .rows_affected();
        if n > 0 {
            added += 1;
            if let Some(hook) = spec.on_change {
                hook(conn, account_id, member, now)
                    .await
                    .map_err(MembershipError::Internal)?;
            }
        }
    }
    let mut removed = 0u64;
    for member in remove {
        let n = sqlx::query(delete_sql.clone())
            .bind(member)
            .bind(id)
            .bind(account_id)
            .execute(&mut *conn)
            .await
            .context("remove a member from a named set")?
            .rows_affected();
        if n > 0 {
            removed += 1;
            if let Some(hook) = spec.on_change {
                hook(conn, account_id, member, now)
                    .await
                    .map_err(MembershipError::Internal)?;
            }
        }
    }
    tx.commit()
        .await
        .context("commit the named set's member changes")?;
    Ok((added, removed))
}

/// Names attached to one member, A–Z.
pub async fn names_for_item(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    item_id: i64,
) -> AnyResult<Vec<String>> {
    let sql = format!(
        "SELECT n.name
         FROM {table} n
         JOIN {members} m ON m.{name_col} = n.id
         WHERE n.account_id = $1 AND m.{member_col} = $2
         ORDER BY lower(n.name)",
        table = spec.table,
        members = spec.members_table,
        name_col = spec.name_column,
        member_col = spec.member_column,
    );
    let rows = sqlx::query_scalar::<_, String>(AssertSqlSafe(sql))
        .bind(account_id)
        .bind(item_id)
        .fetch_all(&mut *conn)
        .await
        .context("read the names of the sets an item is in")?;
    Ok(rows)
}

/// Names attached to each member id, A–Z within each list.
pub async fn names_for_items(
    spec: &'static MembershipSpec,
    conn: &mut SqliteConnection,
    account_id: i64,
    item_ids: &[i64],
) -> AnyResult<std::collections::HashMap<i64, Vec<String>>> {
    use crate::db::sql::{fold_in_id_chunks, in_placeholders};
    #[derive(sqlx::FromRow)]
    struct Row {
        member_id: i64,
        name: String,
    }
    fold_in_id_chunks(conn, item_ids, |conn, chunk| {
        Box::pin(async move {
            let placeholders = in_placeholders(2, chunk.len());
            let sql = format!(
                "SELECT m.{member_col} AS member_id, n.name
                 FROM {members} m
                 JOIN {table} n ON n.id = m.{name_col}
                 WHERE n.account_id = $1 AND m.{member_col} IN ({placeholders})
                 ORDER BY lower(n.name)",
                table = spec.table,
                members = spec.members_table,
                name_col = spec.name_column,
                member_col = spec.member_column,
            );
            let mut q = sqlx::query_as::<_, Row>(AssertSqlSafe(sql)).bind(account_id);
            for id in chunk {
                q = q.bind(*id);
            }
            let rows = q
                .fetch_all(&mut *conn)
                .await
                .context("read the names of the sets each item is in")?;
            Ok(rows
                .into_iter()
                .map(|row| (row.member_id, row.name))
                .collect())
        })
    })
    .await
}

/// Link one member to one name row, doing nothing when the link exists or
/// the member is not the account's. Binds: name row id, member id, account.
fn insert_member_sql(spec: &MembershipSpec) -> SqlStr {
    AssertSqlSafe(format!(
        "INSERT INTO {mt} ({mc}, {nc})
         SELECT id, $1 FROM {member_table} WHERE id = $2 AND account_id = $3
         ON CONFLICT DO NOTHING",
        mt = spec.members_table,
        mc = spec.member_column,
        nc = spec.name_column,
        member_table = spec.member_table,
    ))
    .into_sql_str()
}

/// Unlink one member from one name row when the member is the account's.
/// Binds: member id, name row id, account.
fn delete_member_sql(spec: &MembershipSpec) -> SqlStr {
    AssertSqlSafe(format!(
        "DELETE FROM {mt}
         WHERE {mc} = $1 AND {nc} = $2
           AND EXISTS (
             SELECT 1 FROM {member_table}
             WHERE {member_table}.id = {mt}.{mc}
               AND {member_table}.account_id = $3
           )",
        mt = spec.members_table,
        mc = spec.member_column,
        nc = spec.name_column,
        member_table = spec.member_table,
    ))
    .into_sql_str()
}

#[cfg(test)]
mod tests;
