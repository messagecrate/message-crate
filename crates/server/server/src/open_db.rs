//! The database, opened from its config.
//!
//! Every command line entry point and the HTTP server open the database the
//! same way: the config (with the command line's `--db` already applied, see
//! [`Config::load_with_db`]) names the file, the pool opens it, and the schema is made sure of before anything reads.
//! Only a Message Crate database is opened ([`schema::APPLICATION_ID`]),
//! and only `serve`, `create-database` and `reset-demo` make a new one.
//! [`OpenDb`] is that opened database plus the config it came from, so a
//! caller holds one value and never re-derives the file.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use sqlx::pool::PoolConnection;
use sqlx::{Connection, SqlitePool};

use crate::config::Config;
use crate::db::schema::DatabaseKind;
use crate::db::{account_profile, engine, schema};
use crate::operation_lock::{self, OperationLock};

/// An opened database and the config it was opened from.
#[derive(Debug, Clone)]
pub struct OpenDb {
    /// The config the database was opened from, with every override applied.
    pub cfg: Config,
    /// Connection pool for the database `cfg` names.
    pub db: SqlitePool,
    /// The error SQLite gave when the pool asked for write-ahead logging, in
    /// SQLite's words. `None` when it gave none. `GET /v1/server/settings`
    /// reports it, so Admin Home can name the cause beside the journal mode.
    pub wal_error: Option<Arc<str>>,
}

/// What is at `path`: `None` when no file is there, or else what the file is.
///
/// The file is read through a read-only connection, so a file that is not a
/// Message Crate database is refused before any statement changes it: the
/// server's own pool would switch it to write-ahead logging on open. The
/// file is not yet known to be one the server made, so the connection is
/// set up as the pool's are ([`engine::connect_read_only`]).
///
/// # Errors
///
/// Returns an error, naming the file, when it cannot be read as SQLite.
pub(crate) async fn inspect(path: &Path) -> Result<Option<DatabaseKind>> {
    if !path.exists() {
        return Ok(None);
    }
    let mut conn = engine::connect_read_only(path)
        .await
        .with_context(|| format!("failed to open database {}", path.display()))?;
    let kind = schema::database_kind(&mut conn)
        .await
        .with_context(|| format!("failed to read database {}", path.display()));
    conn.close().await?;
    Ok(Some(kind?))
}

impl OpenDb {
    /// Open the Message Crate database `cfg` names and make sure its schema
    /// is current. This is how every command but `serve` and
    /// `create-database` opens it.
    ///
    /// # Errors
    ///
    /// Returns an error, naming the path, when no file is there or the file
    /// is not a Message Crate database; then nothing is created or changed,
    /// so a mistyped `--db` or another program's SQLite file is left alone.
    /// Also when the file cannot be opened or the schema cannot be applied.
    pub async fn open(cfg: Config) -> Result<Self> {
        match inspect(&cfg.paths.db).await? {
            Some(DatabaseKind::MessageCrate) => Self::open_pool(cfg, None).await,
            Some(DatabaseKind::Empty | DatabaseKind::Foreign) => {
                Err(schema::not_a_message_crate_database(&cfg.paths.db))
            }
            None => bail!(
                "there is no database at {}. Only `serve` and `create-database` make a new \
                 one; this command opens a database that exists",
                cfg.paths.db.display()
            ),
        }
    }

    /// [`OpenDb::open`] for a command that writes files under the Data
    /// Directory, such as `import` and `process-assets`: a missing Data
    /// Directory is made readable by the server's user alone
    /// ([`crate::owner_only`]) before anything is written into it, as when a
    /// database outside it is opened on a new computer.
    ///
    /// # Errors
    ///
    /// Returns what [`OpenDb::open`] does, and an error naming the Data
    /// Directory when it cannot be made.
    pub async fn open_to_write_data_dir(cfg: Config) -> Result<Self> {
        let opened = Self::open(cfg).await?;
        // `open_pool` tightened one that is there.
        file_io::create_dir_all_owner_only(&opened.cfg.paths.data_dir).with_context(|| {
            format!(
                "create data directory {}",
                opened.cfg.paths.data_dir.display()
            )
        })?;
        Ok(opened)
    }

    /// Open the database `cfg` names, making it when no file is there or the
    /// file is empty. A new database's directory is made too, which is how a new
    /// Message Crate begins. `serve` and `create-database` open it this way,
    /// as does `reset-demo`, which builds the Demo Account into a new one.
    ///
    /// # Errors
    ///
    /// Returns an error, naming the path, when the file there is not a
    /// Message Crate database; the file is left as it is. Also when the file
    /// cannot be opened or created, or the schema cannot be applied.
    pub async fn create_or_open(cfg: Config) -> Result<Self> {
        Self::create_or_open_with(cfg, None).await
    }

    /// [`OpenDb::create_or_open`] for `serve`, which already holds the
    /// database's operation lock, so a rebuild for another Schema Fingerprint
    /// goes ahead under that lock instead of taking it again.
    ///
    /// # Errors
    ///
    /// Returns what [`OpenDb::create_or_open`] does.
    pub(crate) async fn create_or_open_holding(cfg: Config, lock: &OperationLock) -> Result<Self> {
        Self::create_or_open_with(cfg, Some(lock)).await
    }

    async fn create_or_open_with(cfg: Config, held: Option<&OperationLock>) -> Result<Self> {
        if inspect(&cfg.paths.db).await? == Some(DatabaseKind::Foreign) {
            return Err(schema::not_a_message_crate_database(&cfg.paths.db));
        }
        if let Some(parent) = cfg.paths.db.parent()
            && !parent.as_os_str().is_empty()
        {
            file_io::create_dir_all_owner_only(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        // Made here and tightened in `open_pool`, so a Data Directory another
        // user owns is warned of once.
        file_io::create_dir_all_owner_only(&cfg.paths.data_dir)
            .with_context(|| format!("create data directory {}", cfg.paths.data_dir.display()))?;
        Self::open_pool(cfg, held).await
    }

    /// Open the pool on the file `cfg` names and make sure of the schema.
    /// The Data Directory is kept readable by the server's user alone first
    /// ([`crate::owner_only`]), whichever command opens it.
    ///
    /// A rebuild for another Schema Fingerprint runs under the database's
    /// operation lock: `held` when the caller has it, or else taken here for
    /// the rebuild, which refuses while a server or reset-demo holds it, so no
    /// command empties the database under a running server (#2920).
    async fn open_pool(cfg: Config, held: Option<&OperationLock>) -> Result<Self> {
        crate::owner_only::keep_data_dir_to_owner(&cfg.paths.data_dir);
        let engine::OpenedPool {
            pool: db,
            wal_error,
        } = engine::open_pool_keeping_wal_error(&cfg.paths.db).await?;
        {
            let mut conn = db.acquire().await?;
            let _rebuild_lock = if held.is_none() && schema::rebuild_is_due(&mut conn).await? {
                Some(operation_lock::acquire_for_rebuild(&cfg.paths.db)?)
            } else {
                None
            };
            schema::ensure_schema(&mut conn).await?;
        }
        Ok(Self { cfg, db, wal_error })
    }

    /// The database file, for status lines and errors.
    pub fn location(&self) -> &Path {
        &self.cfg.paths.db
    }

    /// A connection from the pool.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool cannot hand one out.
    pub async fn conn(&self) -> Result<PoolConnection<sqlx::Sqlite>> {
        Ok(self.db.acquire().await?)
    }

    /// The account id behind a `--account` value: a username, or an id
    /// given as is.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty value or a username the database does not
    /// have.
    pub async fn account_id(&self, account_ref: &str) -> Result<i64> {
        let mut conn = self.conn().await?;
        account_profile::resolve_account_ref(&mut conn, account_ref).await
    }

    /// Close the pool, so a command line run ends with the file released.
    pub async fn close(self) {
        self.db.close().await;
    }
}

/// A config for a fresh database under `dir`, for tests that open one through
/// [`OpenDb`].
#[cfg(test)]
pub(crate) fn fresh_config(dir: &Path) -> Config {
    use crate::config::PathsConfig;

    Config {
        paths: PathsConfig {
            db: dir.join("messagecrate.db"),
            data_dir: dir.join("data"),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        },
        server: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::ConnectOptions;
    use sqlx::sqlite::SqliteConnectOptions;

    #[tokio::test]
    async fn opening_a_new_database_creates_it_with_its_schema() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = fresh_config(dir.path());
        cfg.paths.db = dir.path().join("new/directory/messagecrate.db");
        let opened = OpenDb::create_or_open(cfg).await.unwrap();

        let mut conn = opened.conn().await.unwrap();
        let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(accounts, 1, "only the administrator's row");
    }

    #[tokio::test]
    async fn account_id_resolves_a_username_and_rejects_an_unknown_one() {
        let dir = tempfile::tempdir().unwrap();
        let opened = OpenDb::create_or_open(fresh_config(dir.path()))
            .await
            .unwrap();
        let mut conn = opened.conn().await.unwrap();
        let alice = account_profile::insert_account(&mut conn, "alice", None, None)
            .await
            .unwrap();
        drop(conn);

        assert_eq!(opened.account_id("Alice").await.unwrap(), alice);
        let err = opened.account_id("nobody").await.unwrap_err();
        assert_eq!(
            err.to_string(),
            "account not found: nobody (use an existing username or account id)"
        );
    }

    /// S7-12: a mistyped `--db` names a path where no file is. The command
    /// stops there, naming the path, and creates neither the file nor its
    /// directory.
    #[tokio::test]
    async fn opening_a_path_where_no_file_is_refuses_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = fresh_config(dir.path());
        cfg.paths.db = dir.path().join("mistyped/messagecrate.db");

        let err = OpenDb::open(cfg.clone()).await.unwrap_err();

        assert!(
            err.to_string()
                .contains(&cfg.paths.db.display().to_string()),
            "{err}"
        );
        assert!(!dir.path().join("mistyped").exists());
    }

    /// S7-3: another program's SQLite file is refused by name, by the
    /// commands that open a database and by those that make one, and is
    /// left byte for byte as it was: not rebuilt, and not even switched to
    /// write-ahead logging.
    #[tokio::test]
    async fn another_programs_sqlite_file_is_refused_and_left_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = fresh_config(dir.path());
        cfg.paths.db = dir.path().join("chat.db");
        let mut conn = SqliteConnectOptions::new()
            .filename(&cfg.paths.db)
            .create_if_missing(true)
            .connect()
            .await
            .unwrap();
        sqlx::query("CREATE TABLE message (ROWID INTEGER PRIMARY KEY, text TEXT)")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO message (text) VALUES ('hello from another program')")
            .execute(&mut conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
        let before = std::fs::read(&cfg.paths.db).unwrap();

        for refused in [
            OpenDb::open(cfg.clone()).await,
            OpenDb::create_or_open(cfg.clone()).await,
        ] {
            let err = refused.unwrap_err().to_string();
            assert!(err.contains("chat.db"), "{err}");
            assert!(err.contains("not a Message Crate database"), "{err}");
        }

        assert_eq!(std::fs::read(&cfg.paths.db).unwrap(), before);
    }

    /// A database this server made opens again, and an empty file is not
    /// one: only the commands that make a database build in it.
    #[tokio::test]
    async fn open_takes_a_message_crate_database_and_refuses_an_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = fresh_config(dir.path());
        OpenDb::create_or_open(cfg.clone())
            .await
            .unwrap()
            .close()
            .await;
        OpenDb::open(cfg.clone()).await.unwrap().close().await;

        let mut empty = fresh_config(dir.path());
        empty.paths.db = dir.path().join("empty.db");
        std::fs::write(&empty.paths.db, b"").unwrap();
        assert!(OpenDb::open(empty.clone()).await.is_err());
        OpenDb::create_or_open(empty).await.unwrap().close().await;
    }

    /// Every account's messages are in the database and the Data Directory,
    /// so a new Message Crate makes both readable by the server's user alone,
    /// and the `-wal` and `-shm` files SQLite makes beside the database take
    /// its mode (#2097).
    #[cfg(unix)]
    #[tokio::test]
    async fn a_new_database_and_data_directory_are_the_owner_s_alone() {
        use crate::test_support::permission_bits as mode;

        let dir = tempfile::tempdir().unwrap();
        let mut cfg = fresh_config(dir.path());
        cfg.paths.db = dir.path().join("database/messagecrate.db");
        let opened = OpenDb::create_or_open(cfg.clone()).await.unwrap();
        let mut conn = opened.conn().await.unwrap();
        account_profile::insert_account(&mut conn, "alice", None, None)
            .await
            .unwrap();

        assert_eq!(mode(&cfg.paths.data_dir), 0o700);
        assert_eq!(mode(dir.path().join("database").as_path()), 0o700);
        for suffix in ["", "-wal", "-shm"] {
            let file = dir.path().join(format!("database/messagecrate.db{suffix}"));
            assert_eq!(mode(&file), 0o600, "{}", file.display());
        }
        drop(conn);
        opened.close().await;
    }

    /// A Message Crate made before #2097 has a Data Directory and a
    /// database other users can read. Opening it takes that away.
    #[cfg(unix)]
    #[tokio::test]
    async fn opening_a_database_others_could_read_makes_it_the_owner_s_alone() {
        use crate::test_support::permission_bits as mode;
        use std::os::unix::fs::PermissionsExt as _;

        let open_to_all = |path: &Path, mode| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        let dir = tempfile::tempdir().unwrap();
        let cfg = fresh_config(dir.path());
        OpenDb::create_or_open(cfg.clone())
            .await
            .unwrap()
            .close()
            .await;
        open_to_all(&cfg.paths.data_dir, 0o755);
        open_to_all(&cfg.paths.db, 0o644);

        OpenDb::open(cfg.clone()).await.unwrap().close().await;

        assert_eq!(mode(&cfg.paths.data_dir), 0o700);
        assert_eq!(mode(&cfg.paths.db), 0o600);
    }

    /// `import --db` on a database outside a Data Directory that is not
    /// there yet writes attachments into it, so it is made owner-only first.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_that_writes_files_makes_a_missing_data_directory_the_owner_s_alone() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = fresh_config(dir.path());
        OpenDb::create_or_open(cfg.clone())
            .await
            .unwrap()
            .close()
            .await;
        std::fs::remove_dir(&cfg.paths.data_dir).unwrap();

        OpenDb::open(cfg.clone()).await.unwrap().close().await;
        assert!(
            !cfg.paths.data_dir.exists(),
            "a read-only command makes none"
        );
        OpenDb::open_to_write_data_dir(cfg.clone())
            .await
            .unwrap()
            .close()
            .await;
        assert_eq!(
            crate::test_support::permission_bits(&cfg.paths.data_dir),
            0o700
        );
    }

    /// A database at `cfg`'s path with one account, stamped as another
    /// build of Message Crate would stamp it.
    async fn database_of_another_build(cfg: &Config) {
        let opened = OpenDb::create_or_open(cfg.clone()).await.unwrap();
        let mut conn = opened.conn().await.unwrap();
        account_profile::insert_account(&mut conn, "alice", None, None)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "PRAGMA user_version = {}",
            schema::SCHEMA_FINGERPRINT ^ 1
        )))
        .execute(&mut *conn)
        .await
        .unwrap();
        drop(conn);
        opened.close().await;
    }

    async fn account_count(cfg: &Config) -> i64 {
        let mut conn = engine::connect_read_only(&cfg.paths.db).await.unwrap();
        let count = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
        count
    }

    /// #2920: a command from another build would rebuild the database empty
    /// under a running server, which goes on answering from the empty one.
    /// While the server holds the operation lock, every command refuses,
    /// names the database, says to stop the server, and leaves it as it was.
    #[tokio::test]
    async fn a_rebuild_is_refused_while_a_server_holds_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = fresh_config(dir.path());
        database_of_another_build(&cfg).await;
        // Taken through another file handle, which is how the lock of a
        // server in another process looks from here.
        let server = operation_lock::acquire_for_serve(&cfg.paths.db).unwrap();

        for refused in [
            OpenDb::open(cfg.clone()).await,
            OpenDb::create_or_open(cfg.clone()).await,
        ] {
            let err = refused.unwrap_err().to_string();
            assert!(err.contains(&cfg.paths.db.display().to_string()), "{err}");
            assert!(err.contains("Stop that server"), "{err}");
        }
        assert_eq!(
            account_count(&cfg).await,
            2,
            "the database is as it was (admin and alice)"
        );

        drop(server);
        OpenDb::open(cfg.clone()).await.unwrap().close().await;
        assert_eq!(
            account_count(&cfg).await,
            1,
            "rebuilt once no server holds it, with only the administrator's row"
        );
    }

    /// `serve` holds the lock itself, so its own rebuild goes ahead under
    /// it, and a database of this build opens while a server holds it,
    /// because nothing is rebuilt.
    #[tokio::test]
    async fn the_lock_holder_rebuilds_and_a_current_database_opens_beside_a_server() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = fresh_config(dir.path());
        database_of_another_build(&cfg).await;
        let lock = operation_lock::acquire_for_serve(&cfg.paths.db).unwrap();

        OpenDb::create_or_open_holding(cfg.clone(), &lock)
            .await
            .unwrap()
            .close()
            .await;
        assert_eq!(
            account_count(&cfg).await,
            1,
            "rebuilt with only the administrator's row"
        );
        OpenDb::open(cfg.clone()).await.unwrap().close().await;
    }

    #[tokio::test]
    async fn location_names_the_sqlite_file() {
        let dir = tempfile::tempdir().unwrap();
        let opened = OpenDb::create_or_open(fresh_config(dir.path()))
            .await
            .unwrap();

        assert_eq!(opened.location(), dir.path().join("messagecrate.db"));
        opened.close().await;
    }
}
