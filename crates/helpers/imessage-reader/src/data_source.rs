//! Open macOS `chat.db` or an iOS backup (encrypted backups decrypt to a temp sms.db).

use std::{
    fs::remove_file,
    path::{Path, PathBuf},
};

use crabapple::Backup;
use imessage_database::{tables::table::get_connection, util::platform::Platform};
use rusqlite::Connection;

use crate::{
    backup::{decrypt_backup, get_decrypted_contacts_database, get_decrypted_message_database},
    contacts::{ContactsIndex, DEFAULT_PATH_IOS},
    error::RuntimeError,
    log::emit_log,
    options::ReaderOptions,
};

struct TempDatabase {
    path: PathBuf,
}

impl TempDatabase {
    /// Remember a temp database path so [`Drop`] can delete it.
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Path of the temp database file.
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        if let Err(why) = remove_file(&self.path) {
            emit_log(format!(
                "The temporary copy of the messages database at {} could not be removed: {why}",
                self.path.display(),
            ));
        }
    }
}

pub(crate) struct DataSource {
    messages_connection: Option<Connection>,
    pub contacts_index: ContactsIndex,
    pub backup: Option<Backup>,
    temp_messages_db: Option<TempDatabase>,
}

impl DataSource {
    /// Open the Messages database (and contacts index) for `options`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backup cannot be decrypted or `chat.db` cannot
    /// be opened.
    pub fn from(options: &ReaderOptions) -> Result<Self, RuntimeError> {
        match options.platform {
            Platform::macOS => {
                let messages_path = options.get_db_path();
                let contacts_index =
                    Self::get_contacts_index(options.contacts_path.as_deref()).unwrap_or_default();

                Ok(Self {
                    messages_connection: Some(get_connection(&messages_path)?),
                    contacts_index,
                    backup: None,
                    temp_messages_db: None,
                })
            }
            Platform::iOS => match decrypt_backup(options)? {
                Some(backup) => {
                    let messages_db =
                        TempDatabase::new(get_decrypted_message_database(&backup, options)?);
                    let contacts_path = match get_decrypted_contacts_database(&backup, options) {
                        Ok(path) => Some(path),
                        Err(e) => {
                            emit_log(format!(
                                "The contacts database in the iPhone backup could not be decrypted, \
                                 so no contact is named from it: {e:#}"
                            ));
                            None
                        }
                    };

                    // `ios-backup`'s test
                    // `an_identities_request_decrypts_into_its_scratch_directory_while_it_runs`
                    // relies on this line staying between decrypting the
                    // Contacts database and deleting it: it gives the backup
                    // a device name longer than a pipe holds, so the reader
                    // stops here until the app reads its output, and lists
                    // the scratch directory meanwhile. Moved, that test
                    // times out waiting for the Contacts copy.
                    emit_log(format!(
                        "Decrypted iOS backup: {} (version {})\n",
                        backup.lockdown().device_name,
                        backup.lockdown().product_version,
                    ));

                    let contacts_index =
                        Self::get_contacts_index(contacts_path.as_deref()).unwrap_or_default();

                    if let Some(ref cp) = contacts_path
                        && let Err(e) = remove_file(cp)
                    {
                        emit_log(format!(
                            "The temporary copy of the contacts database at {} could not be removed: {e}",
                            cp.display()
                        ));
                    }

                    let messages_connection = get_connection(messages_db.path())?;
                    Ok(Self {
                        messages_connection: Some(messages_connection),
                        contacts_index,
                        backup: Some(backup),
                        temp_messages_db: Some(messages_db),
                    })
                }
                None => {
                    let messages_path = options.get_db_path();
                    let contacts_index =
                        Self::get_contacts_index(Some(&options.db_path.join(DEFAULT_PATH_IOS)))
                            .unwrap_or_default();

                    Ok(Self {
                        messages_connection: Some(get_connection(&messages_path)?),
                        contacts_index,
                        backup: None,
                        temp_messages_db: None,
                    })
                }
            },
        }
    }

    /// Build a contacts index, or `None` (with a log line) when that fails.
    fn get_contacts_index(path: Option<&Path>) -> Option<ContactsIndex> {
        match ContactsIndex::build(path) {
            Ok(index) => Some(index),
            Err(e) => {
                emit_log(format!(
                    "Contacts could not be read, so no contact is named from them: {e}"
                ));
                None
            }
        }
    }

    /// Whether attachment paths need this process to decrypt them.
    pub fn is_encrypted(&self) -> bool {
        self.backup.as_ref().is_some_and(|b| b.is_encrypted())
    }

    /// Open SQLite connection to the Messages database.
    pub fn db(&self) -> &Connection {
        match self.messages_connection.as_ref() {
            Some(db) => db,
            None => panic!("Database connection is closed!"),
        }
    }
}

impl Drop for DataSource {
    fn drop(&mut self) {
        if let Some(conn) = self.messages_connection.take() {
            conn.close().ok();
        }
        drop(self.temp_messages_db.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        error::IOS_BACKUP_PASSWORD_INCORRECT,
        session::MailSession,
        test_support::{FixtureBackup, FixtureDb, mac_source},
    };
    use chat_db_fixture::{
        FRIEND_PHONE,
        ios_backup::{BACKUP_PASSWORD, Encryption},
    };
    use imessage_reader_protocol::ExportRequest;
    use std::fs;

    /// A Mac `chat.db` opens in place: no backup, nothing decrypted, nothing
    /// to clean up, and the connection answers queries.
    #[test]
    fn a_mac_database_opens_in_place() {
        let fixture = FixtureDb::write();
        let source = DataSource::from(&fixture.options()).unwrap();
        assert!(!source.is_encrypted());
        assert!(source.backup.is_none());
        assert!(source.temp_messages_db.is_none());
        let chats: i64 = source
            .db()
            .query_row("SELECT count(*) FROM chat", [], |row| row.get(0))
            .unwrap();
        assert_eq!(chats, 6);
    }

    /// A contacts file that cannot be read is logged and the run goes on
    /// without names, rather than failing the whole export.
    #[test]
    fn an_unreadable_contacts_file_leaves_the_index_empty() {
        let fixture = FixtureDb::write();
        let not_a_db = fixture.dir.path().join("contacts.db");
        std::fs::write(&not_a_db, b"not sqlite").unwrap();
        let options = ReaderOptions::from_export(ExportRequest {
            contacts_path: Some(not_a_db),
            ..fixture.export_request()
        });
        let source = DataSource::from(&options).unwrap();
        assert_eq!(source.contacts_index.lookup("+15555550107"), None);
        assert!(
            DataSource::get_contacts_index(Some(&fixture.dir.path().join("missing.db"))).is_none()
        );
    }

    /// A database that is not there is the error the app shows, not a panic.
    #[test]
    fn a_missing_database_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let options =
            ReaderOptions::from_source(mac_source(&dir.path().join("chat.db")), dir.path().into());
        assert!(DataSource::from(&options).is_err());
    }

    /// The encrypted backup opens with its password: the decrypted Messages
    /// database is the one file in the request's scratch directory while
    /// the source is open, its messages come out of the reader, the
    /// decrypted Contacts database names a sender and is already gone, the
    /// backup is left as it was, and closing the source empties the scratch
    /// directory (#788, #1386).
    #[test]
    fn an_encrypted_backup_decrypts_into_the_scratch_directory_and_nowhere_else() {
        let fixture = FixtureBackup::write(Encryption::Password(BACKUP_PASSWORD));
        let before = fixture.backup_listing();
        let session = MailSession::new(fixture.options(Some(BACKUP_PASSWORD))).unwrap();
        let source = &session.data_source;

        assert!(source.is_encrypted());
        let scratch = fixture.scratch_files();
        assert_eq!(scratch.len(), 1, "{scratch:?}");
        assert!(
            scratch[0].starts_with("crabapple-sms-") && scratch[0].ends_with(".db"),
            "{scratch:?}"
        );
        let decrypted = fixture.scratch.path().join(&scratch[0]);
        assert!(
            fs::read(&decrypted)
                .unwrap()
                .starts_with(b"SQLite format 3\0")
        );

        let texts: Vec<String> = FixtureDb::messages(&session)
            .into_iter()
            .filter_map(|message| message.text)
            .collect();
        assert!(texts.iter().any(|t| t == "Saturday works"), "{texts:?}");
        assert_eq!(
            source
                .contacts_index
                .lookup(FRIEND_PHONE)
                .map(|name| name.full),
            Some("Sam Example".to_string()),
            "the contacts database was decrypted and read"
        );
        assert_eq!(fixture.backup_listing(), before);

        drop(session);
        assert_eq!(fixture.scratch_files(), Vec::<String>::new());
    }

    /// The backup that is not encrypted opens without a password and is read
    /// where it is: nothing is decrypted and nothing is written to the
    /// scratch directory, and its contacts still name a sender.
    #[test]
    fn an_unencrypted_backup_opens_without_a_password() {
        let fixture = FixtureBackup::write(Encryption::None);
        let source = DataSource::from(&fixture.options(None)).unwrap();

        assert!(!source.is_encrypted());
        assert!(source.backup.is_none());
        assert!(source.temp_messages_db.is_none());
        let chats: i64 = source
            .db()
            .query_row("SELECT count(*) FROM chat", [], |row| row.get(0))
            .unwrap();
        assert_eq!(chats, 6);
        assert_eq!(
            source
                .contacts_index
                .lookup(FRIEND_PHONE)
                .map(|name| name.full),
            Some("Sam Example".to_string())
        );
        assert_eq!(fixture.scratch_files(), Vec::<String>::new());
    }

    /// A wrong password is refused with the reader's own message, and
    /// nothing is written to the scratch directory.
    #[test]
    fn a_wrong_password_is_refused_and_nothing_is_decrypted() {
        let fixture = FixtureBackup::write(Encryption::Password(BACKUP_PASSWORD));
        let Err(err) = DataSource::from(&fixture.options(Some("not-the-password"))) else {
            panic!("a wrong password opened the backup");
        };
        assert_eq!(err.to_string(), IOS_BACKUP_PASSWORD_INCORRECT);
        assert_eq!(fixture.scratch_files(), Vec::<String>::new());
    }

    /// A temp database is deleted when its handle drops, and a path that is
    /// already gone only logs.
    #[test]
    fn a_temp_database_is_removed_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sms.db");
        std::fs::write(&path, b"bytes").unwrap();
        let temp = TempDatabase::new(path.clone());
        assert_eq!(temp.path(), path);
        drop(temp);
        assert!(!path.exists());
        drop(TempDatabase::new(path));
    }
}
