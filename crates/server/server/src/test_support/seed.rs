//! Rows and stored values a test writes straight into the database or the
//! asset store, without going through a route or an import.

use crate::server::AppState;

/// Link the `handles` row `handle_id` to `account_id` as one of its
/// identities, as adding one in Settings does.
pub async fn link_identity(conn: &mut sqlx::SqliteConnection, account_id: i64, handle_id: i64) {
    sqlx::query("INSERT INTO account_handles (account_id, handle_id) VALUES ($1, $2)")
        .bind(account_id)
        .bind(handle_id)
        .execute(&mut *conn)
        .await
        .unwrap();
}

/// Run `edit` on `conn` with the schema's trigger `trigger` lifted, then put
/// it back as the schema wrote it. For a test that makes a write the schema
/// refuses, such as standing an Audit Trail entry in the past
/// (`audit_entries_never_edited`), which the server never does.
pub async fn with_trigger_lifted<T>(
    conn: &mut sqlx::SqliteConnection,
    trigger: &str,
    edit: impl AsyncFnOnce(&mut sqlx::SqliteConnection) -> T,
) -> T {
    let sql: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = $1")
            .bind(trigger)
            .fetch_one(&mut *conn)
            .await
            .unwrap_or_else(|e| panic!("read trigger {trigger}: {e:#}"));
    sqlx::raw_sql(&format!("DROP TRIGGER {trigger}"))
        .execute(&mut *conn)
        .await
        .unwrap();
    let result = edit(&mut *conn).await;
    sqlx::raw_sql(&sql).execute(&mut *conn).await.unwrap();
    result
}

/// Insert an `accounts` row with a chosen id on `conn`, with no password
/// and no preferred name, for a test that has a connection rather than a
/// [`TestFixture`]. Returns the id it was given.
pub async fn insert_account_with_id(
    conn: &mut sqlx::SqliteConnection,
    id: i64,
    username: &str,
) -> i64 {
    crate::db::account_profile::insert_account_at(conn, id, username, None, None)
        .await
        .unwrap_or_else(|e| panic!("insert account {username} at id {id}: {e:#}"));
    id
}

/// Store an attachment size limit directly, the way a test lowers the body
/// cap below the part size.
pub async fn store_asset_max_bytes(state: &AppState, bytes: u64) {
    let mut conn = state.db.acquire().await.unwrap();
    crate::db::server_settings::set_asset_max_bytes(&mut conn, bytes)
        .await
        .unwrap();
}

/// One message to seed into a conversation.
pub struct SeedMessage<'a> {
    /// The `messages.source` slug, such as `imessage`.
    pub source: &'a str,
    /// When it was sent, in the stored form ([`stored_time`]).
    pub timestamp: crate::models::StoredTime,
    /// Whether the account sent it.
    pub is_from_me: bool,
    /// The message text.
    pub body: &'a str,
}

/// A conversation to seed, with its messages in order.
pub struct SeedConversation<'a> {
    /// The account that owns it.
    pub account_id: i64,
    /// The peer handle, created as a `handles` row. Must be unique per
    /// account: `handles` is keyed on the normalized value.
    pub handle: &'a str,
    /// `individual` or `group`.
    pub conversation_type: &'a str,
    /// The group's title, for a group conversation.
    pub group_title: Option<&'a str>,
    /// The `conversations.source_file` value.
    pub source_file: &'a str,
    /// Messages, seeded with `sort_order` following this order.
    pub messages: &'a [SeedMessage<'a>],
}

/// The stored form of the RFC 3339 time `rfc3339`, made by the one function
/// that writes it (`models::utc_timestamp_text`), so a test seeds a time the
/// way an import stores it whatever form it spells it in (#1965). Panics on
/// text that is not RFC 3339.
pub fn stored_time(rfc3339: &str) -> crate::models::StoredTime {
    let instant = chrono::DateTime::parse_from_rfc3339(rfc3339)
        .unwrap_or_else(|e| panic!("{rfc3339:?} is not RFC 3339: {e}"))
        .with_timezone(&chrono::Utc);
    crate::models::utc_timestamp_text(instant)
}

/// A message guid no earlier call returned. `messages.guid` is required and
/// unique per account and source, so [`MessageRow::new`] takes a fresh one
/// from here for every row; a test that names its message sets a literal.
pub fn unique_guid() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!(
        "test-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

/// One `messages` row for a test to insert: the one place the server's tests
/// write a message by hand, so a new required column is one edit here.
///
/// [`MessageRow::new`] fills every required column: source `imessage`, a guid
/// from [`unique_guid`], the first instant of 2020, received, and
/// `sort_order` 0. A test sets what it cares about with struct update
/// syntax:
///
/// ```ignore
/// MessageRow { body: Some("hello"), ..MessageRow::new(account_id, conversation_id) }
///     .insert(&mut conn)
///     .await;
/// ```
///
/// [`MessageRow::insert`] writes it in a write transaction of its own, as
/// every write to `messages` must be (`crate::db::write_guard`).
#[derive(Debug, Clone)]
pub struct MessageRow<'a> {
    /// `messages.id`; `None` lets SQLite choose it.
    pub id: Option<i64>,
    /// `messages.conversation_id`.
    pub conversation_id: i64,
    /// `messages.account_id`.
    pub account_id: i64,
    /// `messages.source`, such as `imessage`.
    pub source: &'a str,
    /// `messages.guid`. `None` writes NULL, which the table refuses.
    pub guid: Option<String>,
    /// When it was sent, in the stored form ([`stored_time`]).
    pub timestamp: crate::models::StoredTime,
    /// `messages.time_precision`.
    pub time_precision: message_ir::TimePrecision,
    /// Whether the account sent it.
    pub is_from_me: bool,
    /// `messages.sender_handle_id`.
    pub sender_handle_id: Option<i64>,
    /// `messages.owner_handle_id`.
    pub owner_handle_id: Option<i64>,
    /// `messages.service`.
    pub service: Option<&'a str>,
    /// `messages.subject`.
    pub subject: Option<&'a str>,
    /// `messages.body`.
    pub body: Option<&'a str>,
    /// `messages.is_announcement`.
    pub is_announcement: bool,
    /// `messages.is_reply`.
    pub is_reply: bool,
    /// `messages.reply_to_guid`.
    pub reply_to_guid: Option<&'a str>,
    /// `messages.reply_to_part`.
    pub reply_to_part: Option<i64>,
    /// `messages.deletion`.
    pub deletion: Option<message_ir::Deletion>,
    /// `messages.sort_order`.
    pub sort_order: i64,
    /// `messages.content_key`.
    pub content_key: Option<&'a str>,
    /// `messages.duplicate_of`.
    pub duplicate_of: Option<i64>,
    /// `messages.import_id`.
    pub import_id: Option<i64>,
}

impl MessageRow<'_> {
    /// A received `imessage` message in `conversation_id` of `account_id`,
    /// with a fresh guid, at 2020-01-01T00:00:00.000Z, and nothing optional set.
    pub fn new(account_id: i64, conversation_id: i64) -> Self {
        Self {
            id: None,
            conversation_id,
            account_id,
            source: "imessage",
            guid: Some(unique_guid()),
            timestamp: stored_time("2020-01-01T00:00:00.000Z"),
            time_precision: message_ir::TimePrecision::Milliseconds,
            is_from_me: false,
            sender_handle_id: None,
            owner_handle_id: None,
            service: None,
            subject: None,
            body: None,
            is_announcement: false,
            is_reply: false,
            reply_to_guid: None,
            reply_to_part: None,
            deletion: None,
            sort_order: 0,
            content_key: None,
            duplicate_of: None,
            import_id: None,
        }
    }

    /// Insert the row in a write transaction of its own on `conn`, and
    /// answer its `messages.id`.
    pub async fn insert(&self, conn: &mut sqlx::SqliteConnection) -> i64 {
        let mut tx = crate::db::begin_write(conn)
            .await
            .expect("begin a write to insert a message");
        let id = self.insert_in(&mut tx).await;
        tx.commit().await.expect("commit the inserted message");
        id
    }

    /// Insert the row in `tx`, the caller's write transaction, and answer
    /// its `messages.id`.
    pub async fn insert_in(&self, tx: &mut crate::db::WriteTx<'_>) -> i64 {
        self.try_insert_in(tx)
            .await
            .unwrap_or_else(|e| panic!("insert message {:?}: {e}", self.guid))
    }

    /// Insert the row in `tx`, and answer its `messages.id` or the error the
    /// table gave, for a test of what the table refuses.
    pub async fn try_insert_in(&self, tx: &mut crate::db::WriteTx<'_>) -> sqlx::Result<i64> {
        sqlx::query_scalar(
            "INSERT INTO messages (
                id, conversation_id, account_id, source, guid, timestamp, is_from_me,
                sender_handle_id, owner_handle_id, service, subject, body,
                is_announcement, is_reply, reply_to_guid, reply_to_part,
                deletion, sort_order, content_key, duplicate_of, import_id,
                time_precision
             ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                $13, $14, $15, $16, $17, $18, $19, $20, $21, $22
             ) RETURNING id",
        )
        .bind(self.id)
        .bind(self.conversation_id)
        .bind(self.account_id)
        .bind(self.source)
        .bind(&self.guid)
        .bind(&self.timestamp)
        .bind(self.is_from_me)
        .bind(self.sender_handle_id)
        .bind(self.owner_handle_id)
        .bind(self.service)
        .bind(self.subject)
        .bind(self.body)
        .bind(self.is_announcement)
        .bind(self.is_reply)
        .bind(self.reply_to_guid)
        .bind(self.reply_to_part)
        .bind(self.deletion.map(message_ir::Deletion::as_str))
        .bind(self.sort_order)
        .bind(self.content_key)
        .bind(self.duplicate_of)
        .bind(self.import_id)
        .bind(self.time_precision.as_str())
        .fetch_one(&mut **tx)
        .await
    }
}

/// One `conversations` row for a test to insert: the one place the server's
/// tests write a conversation by hand, so a new required column is one edit
/// here.
///
/// [`ConversationRow::new`] fills every required column: an `individual`
/// conversation from `test.jsonl` with no group title. A test sets what it
/// cares about with struct update syntax:
///
/// ```ignore
/// ConversationRow { id: Some(2), ..ConversationRow::new(account_id, chat_handle_id) }
///     .insert(&mut conn)
///     .await;
/// ```
#[derive(Debug, Clone)]
pub struct ConversationRow<'a> {
    /// `conversations.id`; `None` lets SQLite choose it.
    pub id: Option<i64>,
    /// `conversations.account_id`.
    pub account_id: i64,
    /// `conversations.chat_handle_id`: the peer's handle for a 1:1
    /// conversation, the group chat's handle for a group.
    pub chat_handle_id: i64,
    /// `conversations.conversation_type`: `individual` or `group`.
    pub conversation_type: &'a str,
    /// `conversations.group_title`.
    pub group_title: Option<&'a str>,
    /// `conversations.source_file`.
    pub source_file: &'a str,
}

impl<'a> ConversationRow<'a> {
    /// An `individual` conversation of `account_id` whose chat handle is
    /// `chat_handle_id`, from `test.jsonl`, with no group title.
    pub fn new(account_id: i64, chat_handle_id: i64) -> Self {
        Self {
            id: None,
            account_id,
            chat_handle_id,
            conversation_type: "individual",
            group_title: None,
            source_file: "test.jsonl",
        }
    }

    /// A `group` conversation of `account_id` titled `title`, whose chat
    /// handle is `chat_handle_id`.
    pub fn group(account_id: i64, chat_handle_id: i64, title: &'a str) -> Self {
        Self {
            conversation_type: "group",
            group_title: Some(title),
            ..Self::new(account_id, chat_handle_id)
        }
    }

    /// Insert the row on `conn`, and answer its `conversations.id`.
    pub async fn insert(&self, conn: &mut sqlx::SqliteConnection) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO conversations (
                id, account_id, chat_handle_id, conversation_type, group_title, source_file
             ) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        )
        .bind(self.id)
        .bind(self.account_id)
        .bind(self.chat_handle_id)
        .bind(self.conversation_type)
        .bind(self.group_title)
        .bind(self.source_file)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|e| panic!("insert conversation {self:?}: {e}"))
    }
}

/// Seed one conversation and its messages, returning the new
/// `conversations.id`.
///
/// `messages.conversation_id` and `conversations.chat_handle_id` are integer
/// foreign keys, so this first creates a `handles` row the way every real
/// importer does rather than binding a string straight into `chat_handle_id`.
pub async fn seed_conversation(state: &AppState, c: &SeedConversation<'_>) -> i64 {
    let mut conn = state.db.acquire().await.unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', 'phone') RETURNING id",
    )
    .bind(c.account_id)
    .bind(c.handle)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let conversation_id = ConversationRow {
        conversation_type: c.conversation_type,
        group_title: c.group_title,
        source_file: c.source_file,
        ..ConversationRow::new(c.account_id, handle_id)
    }
    .insert(&mut conn)
    .await;

    for (index, message) in c.messages.iter().enumerate() {
        MessageRow {
            source: message.source,
            timestamp: message.timestamp.clone(),
            is_from_me: message.is_from_me,
            sort_order: index as i64,
            body: Some(message.body),
            ..MessageRow::new(c.account_id, conversation_id)
        }
        .insert(&mut conn)
        .await;
    }

    conversation_id
}

/// Store a real attachment file for the account's `imessage` source and
/// attach it to the newest message of `conversation_id`, returning the
/// file's path so a test can check whether a delete removed it. The MIME
/// sidecar the store writes beside a file whose type it knows is written
/// too, so the same test can check that it went with the file. The file is
/// named by `sha` alone, with no extension, as the store names an Asset.
///
/// `sha` stands in for the content hash; the store never reads the bytes
/// back here, so it only has to be 64 hex characters the way a real digest
/// is. The file goes in the account's one assets directory, the directory
/// every source of the account shares, which is where a delete looks for it.
pub async fn attach_stored_file(
    state: &AppState,
    account_id: i64,
    conversation_id: i64,
    sha: &str,
) -> std::path::PathBuf {
    let assets_dir = state.cfg.paths.assets_dir_for_account(account_id);
    let (sha, rel, path) =
        write_stored_original(&assets_dir, sha, "", Some("image/jpeg"), b"jpeg bytes");

    let mut conn = state.db.acquire().await.unwrap();
    let message_id: i64 = sqlx::query_scalar(
        "SELECT id FROM messages WHERE conversation_id = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind(conversation_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
    sqlx::query("INSERT INTO attachments (message_id, sha256, assets_path) VALUES ($1, $2, $3)")
        .bind(message_id)
        .bind(sha.as_str())
        .bind(rel)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    path
}

/// Write `bytes` in `assets_dir` where the store places the original of
/// `sha`, with `suffix` after the file name, and its MIME sidecar when `mime`
/// is given. Both paths come from the store's own rules (`shard_rel_path`,
/// `sidecar_path`) for the parsed fingerprint. Returns that fingerprint, the
/// original's path relative to `assets_dir` for the attachment row, and the
/// path of the file written.
pub(crate) fn write_stored_original(
    assets_dir: &std::path::Path,
    sha: &str,
    suffix: &str,
    mime: Option<&str>,
    bytes: &[u8],
) -> (crate::assets_api::Sha256, String, std::path::PathBuf) {
    let sha = crate::assets_api::Sha256::parse(sha)
        .expect("write_stored_original needs a 64-hex fingerprint");
    let rel = crate::assets_api::shard_rel_path(&sha, suffix);
    let path = assets_dir.join(&rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    if let Some(mime) = mime {
        std::fs::write(crate::asset_store::sidecar_path(assets_dir, &sha), mime).unwrap();
    }
    (sha, rel, path)
}

/// 64 hex-looking characters, distinct per `tag`: the length of a SHA-256
/// digest, for a test that stores a file under a fingerprint of its choosing.
pub fn fake_sha256(tag: char) -> String {
    std::iter::repeat_n(tag, 64).collect()
}

/// Give an account one conversation holding one message, so counts are
/// non-zero.
pub async fn seed_one_message(state: &AppState, account_id: i64) {
    seed_conversation(
        state,
        &SeedConversation {
            account_id,
            handle: &format!("+1555{account_id}"),
            conversation_type: "individual",
            group_title: None,
            source_file: "seed.jsonl",
            messages: &[SeedMessage {
                source: "imessage",
                timestamp: stored_time("2020-01-01T00:00:00.000Z"),
                is_from_me: true,
                body: "hello",
            }],
        },
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_fixture;

    #[tokio::test]
    async fn the_seeder_returns_the_conversation_id_it_made() {
        let fixture = test_fixture().await;
        let account = fixture.account("alice").await;
        let id = seed_conversation(
            &fixture.state,
            &SeedConversation {
                account_id: account,
                handle: "+15555550100",
                conversation_type: "group",
                group_title: Some("Book Club"),
                source_file: "backup-a.jsonl",
                messages: &[
                    SeedMessage {
                        source: "imessage",
                        timestamp: stored_time("2020-01-01T00:00:00.000Z"),
                        is_from_me: true,
                        body: "first",
                    },
                    SeedMessage {
                        source: "imessage",
                        timestamp: stored_time("2020-01-02T00:00:00.000Z"),
                        is_from_me: false,
                        body: "second",
                    },
                ],
            },
        )
        .await;

        let mut conn = fixture.conn().await;
        let title: String =
            sqlx::query_scalar("SELECT group_title FROM conversations WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(title, "Book Club");

        let bodies: Vec<String> = sqlx::query_scalar(
            "SELECT body FROM messages WHERE conversation_id = $1 ORDER BY sort_order",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        assert_eq!(bodies, vec!["first".to_string(), "second".to_string()]);
    }
}
