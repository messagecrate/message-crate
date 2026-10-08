//! An iPhone backup made from the fixture `chat.db`, small and made up, in
//! the two forms a phone writes: as it is, and encrypted with a password.
//!
//! The encrypted form follows Apple's layout as `crabapple` reads it, and the
//! `imessage-reader` helper decrypts it with `crabapple`:
//!
//! - `Manifest.plist` holds a key bag. The password goes through two PBKDF2
//!   passes (HMAC-SHA256 over `DPSL`, then HMAC-SHA1 over `SALT`) to the key
//!   that unwraps the protection class key (AES key wrap, RFC 3394).
//! - `Manifest.db` is encrypted under its own key, which `Manifest.plist`
//!   holds wrapped with the class key.
//! - Every file is encrypted under a key of its own (AES-256-CBC, zero IV,
//!   PKCS#7 padding), held wrapped with the class key in the file's
//!   `Manifest.db` row.
//!
//! The encryption is written here with the RustCrypto crates `crabapple`
//! itself depends on, not with `crabapple`, so this crate stays permissive
//! and the exporter's tests link no GPL code
//! (`docs/adr/0014-gpl-code-only-behind-a-process-boundary.md`).
//!
//! Nothing here comes from a real backup. The key bag's iteration counts
//! are 2, not the millions a phone uses, so a test opens it in milliseconds.

use std::{fs, path::Path, path::PathBuf};

use aes::{
    Aes256,
    cipher::{BlockModeEncrypt, KeyIvInit, block_padding::Pkcs7},
};
use aes_kw::{KeyInit, KwAes256};
use plist::{Dictionary, Uid, Value};
use rusqlite::Connection;
use sha1::{Digest, Sha1};

use crate::{PHOTO_BYTES, write_chat_db};

/// The password the encrypted backup is made with.
pub const BACKUP_PASSWORD: &str = "fixture-password";

/// The domain an iPhone keeps Messages and Contacts in.
pub const HOME_DOMAIN: &str = "HomeDomain";

/// The domain an iPhone keeps Messages attachments in.
pub const MEDIA_DOMAIN: &str = "MediaDomain";

/// Where the Messages database is inside [`HOME_DOMAIN`].
pub const MESSAGES_DB_PATH: &str = "Library/SMS/sms.db";

/// Where the Contacts database is inside [`HOME_DOMAIN`].
pub const CONTACTS_DB_PATH: &str = "Library/AddressBook/AddressBook.sqlitedb";

/// Where the photo is inside [`MEDIA_DOMAIN`]. The `attachment.filename`
/// an iPhone writes is this path after `~/`.
pub const PHOTO_PATH: &str = "Library/SMS/Attachments/00/00/att-1/photo.jpg";

/// The name `crabapple` gives the decrypted `Manifest.db` of an encrypted
/// backup it opens. It writes the file into the process's temporary
/// directory (`std::env::temp_dir`) and removes it when the backup is
/// dropped.
pub const DECRYPTED_MANIFEST_NAME: &str = "crabapple-Manifest.db";

/// A turn at the system's temporary directory, held while a test opens an
/// encrypted backup in process or checks that directory for
/// [`DECRYPTED_MANIFEST_NAME`].
///
/// `imessage-reader`'s own tests open encrypted backups in process, where
/// nothing moves the temporary directory, so each writes its decrypted
/// `Manifest.db` to the one fixed path in the system's temporary directory.
/// Two such tests at once write over each other's copy, and a test that
/// checks the reader program leaves nothing there would see theirs. Test
/// threads of one binary, and test binaries side by side under
/// cargo-nextest or in two worktrees, all lock one file in the system's
/// temporary directory, so they take turns.
pub struct OneOpenBackup {
    /// The locked file; dropping it lets the next test go.
    _locked: fs::File,
}

/// Wait for the turn at the system's temporary directory, and hold it
/// until the value drops.
///
/// # Panics
///
/// Panics when the lock file cannot be created or locked.
#[must_use]
pub fn one_open_backup_at_a_time() -> OneOpenBackup {
    let file = fs::File::create(std::env::temp_dir().join("chat-db-fixture-backup.lock"))
        .expect("create the backup lock file");
    file.lock().expect("lock the backup lock file");
    OneOpenBackup { _locked: file }
}

/// Whether, and how, a backup is encrypted.
#[derive(Debug, Clone, Copy)]
pub enum Encryption<'a> {
    /// Files are stored as they are, and `Manifest.db` is plain SQLite.
    None,
    /// Every file and `Manifest.db` are encrypted under keys this password
    /// unlocks.
    Password(&'a str),
}

/// One file of a made-up backup.
#[derive(Debug, Clone, Copy)]
pub struct BackupFile<'a> {
    /// The domain, such as [`HOME_DOMAIN`].
    pub domain: &'a str,
    /// The path inside the domain.
    pub relative_path: &'a str,
    /// The file's bytes. `None` lists the file in `Manifest.db` and leaves
    /// its bytes out of the backup.
    pub bytes: Option<&'a [u8]>,
}

/// The id a backup gives a file: the SHA-1 of `<domain>-<relative path>`, in
/// lower-case hex. The file is stored at `<first two characters>/<id>`.
#[must_use]
pub fn file_id(domain: &str, relative_path: &str) -> String {
    Sha1::digest(format!("{domain}-{relative_path}").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Where a backup rooted at `root` stores the file `domain` holds at
/// `relative_path`.
#[must_use]
pub fn stored_path(root: &Path, domain: &str, relative_path: &str) -> PathBuf {
    let id = file_id(domain, relative_path);
    root.join(&id[..2]).join(id)
}

/// Write an iPhone backup into the empty directory `root` holding the
/// fixture `chat.db` ([`write_chat_db`]) as the Messages database, a
/// Contacts database ([`fill_ios_address_book`]), and the photo
/// ([`PHOTO_BYTES`]) at [`PHOTO_PATH`], whose `attachment.filename` is the
/// `~/`-prefixed path an iPhone writes.
///
/// # Panics
///
/// Panics when a file cannot be written; a test has nothing better to do.
pub fn write_messages_backup(root: &Path, encryption: Encryption<'_>) {
    let work = root.join(".fixture-work");
    fs::create_dir_all(&work).expect("make the fixture's work directory");
    let chat_db = write_chat_db(&work);
    Connection::open(&chat_db)
        .expect("open chat.db")
        .execute(
            "UPDATE attachment SET filename = ?1",
            [format!("~/{PHOTO_PATH}")],
        )
        .expect("point the attachment at the backup's photo");
    let contacts_db = work.join("AddressBook.sqlitedb");
    fill_ios_address_book(&Connection::open(&contacts_db).expect("create the address book"));

    let messages = fs::read(&chat_db).expect("read chat.db");
    let contacts = fs::read(&contacts_db).expect("read the address book");
    fs::remove_dir_all(&work).expect("remove the fixture's work directory");

    write_backup(
        root,
        &[
            BackupFile {
                domain: HOME_DOMAIN,
                relative_path: MESSAGES_DB_PATH,
                bytes: Some(&messages),
            },
            BackupFile {
                domain: HOME_DOMAIN,
                relative_path: CONTACTS_DB_PATH,
                bytes: Some(&contacts),
            },
            BackupFile {
                domain: MEDIA_DOMAIN,
                relative_path: PHOTO_PATH,
                bytes: Some(PHOTO_BYTES),
            },
        ],
        encryption,
    );
}

/// The full-text table an iPhone's `AddressBook.sqlitedb` holds, with the
/// space-separated phone and email columns the reader splits: "Sam Example"
/// at [`crate::FRIEND_PHONE`] and `sam@example.com`, "Robin" at
/// [`crate::FRIEND_EMAIL`], and a nameless row at `+15555550179`.
///
/// # Panics
///
/// Panics when the table cannot be written.
pub fn fill_ios_address_book(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE ABPersonFullTextSearch_content (c0First TEXT, c1Last TEXT, c16Phone TEXT, c17Email TEXT);
         INSERT INTO ABPersonFullTextSearch_content VALUES ('Sam', 'Example', '+15555550107 15555550107 5555550107', 'Sam@Example.com sam@work.example');
         INSERT INTO ABPersonFullTextSearch_content VALUES ('Robin', NULL, NULL, 'friend@example.com');
         INSERT INTO ABPersonFullTextSearch_content VALUES (NULL, NULL, '+15555550179', NULL);",
    )
    .expect("fill the iOS address book");
}

/// Write a backup of `files` into the empty directory `root`:
/// `Manifest.plist`, `Manifest.db` listing every file as a regular file, and
/// each file that has bytes at [`stored_path`].
///
/// # Panics
///
/// Panics when a file cannot be written.
pub fn write_backup(root: &Path, files: &[BackupFile<'_>], encryption: Encryption<'_>) {
    let keys = match encryption {
        Encryption::None => None,
        Encryption::Password(password) => Some(Keys::new(password)),
    };
    write_manifest_plist(root, keys.as_ref());

    let plain_manifest = root.join("Manifest.plain.db");
    let manifest = Connection::open(&plain_manifest).expect("create Manifest.db");
    manifest
        .execute_batch(
            "CREATE TABLE Files (fileID TEXT PRIMARY KEY, domain TEXT, relativePath TEXT,
                                 flags INTEGER, file BLOB);",
        )
        .expect("create the Files table");
    for (index, file) in files.iter().enumerate() {
        let id = file_id(file.domain, file.relative_path);
        let file_key = file_key(index);
        let size = file.bytes.map_or(1, <[u8]>::len) as u64;
        let metadata = keys.as_ref().map(|keys| keys.metadata(size, &file_key));
        manifest
            .execute(
                "INSERT INTO Files VALUES (?1, ?2, ?3, 1, ?4)",
                (&id, file.domain, file.relative_path, metadata),
            )
            .expect("list the file");
        if let Some(bytes) = file.bytes {
            let stored = stored_path(root, file.domain, file.relative_path);
            fs::create_dir_all(stored.parent().expect("a stored file has a directory"))
                .expect("make the file's directory");
            let stored_bytes = match keys {
                Some(_) => aes_cbc_encrypt(bytes, &file_key),
                None => bytes.to_vec(),
            };
            fs::write(stored, stored_bytes).expect("write the file");
        }
    }
    drop(manifest);

    let plain = fs::read(&plain_manifest).expect("read Manifest.db");
    fs::remove_file(&plain_manifest).expect("remove the plain Manifest.db");
    let stored = match keys {
        Some(_) => aes_cbc_encrypt(&plain, &MANIFEST_KEY),
        None => plain,
    };
    fs::write(root.join("Manifest.db"), stored).expect("write Manifest.db");
}

/// The protection class every file is in ("protected until first user
/// authentication", the class most of a phone's files are in).
const CLASS: u32 = 3;

/// The class key, which the password-derived key wraps.
const CLASS_KEY: [u8; 32] = [0x11; 32];

/// The key `Manifest.db` is encrypted under.
const MANIFEST_KEY: [u8; 32] = [0x22; 32];

/// The salt and iteration count of the first PBKDF2 pass (HMAC-SHA256).
const DPSL: &[u8] = b"fixture-dpsl";
const DPIC: u32 = 2;

/// The salt and iteration count of the second PBKDF2 pass (HMAC-SHA1).
const SALT: &[u8] = b"fixture-salt";
const ITER: u32 = 2;

/// The key file `index` is encrypted under: each file has its own.
fn file_key(index: usize) -> [u8; 32] {
    let mut key = [0x33; 32];
    key[..8].copy_from_slice(&(index as u64).to_be_bytes());
    key
}

/// The keys of an encrypted backup, from its password.
struct Keys {
    /// The key the password derives, which unwraps the class key.
    password_key: [u8; 32],
}

impl Keys {
    fn new(password: &str) -> Self {
        let mut first = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), DPSL, DPIC, &mut first);
        let mut password_key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha1>(&first, SALT, ITER, &mut password_key);
        Self { password_key }
    }

    /// The `BackupKeyBag` of `Manifest.plist`: a run of tag, big-endian
    /// length, value. The header says the bag is a backup bag (`TYPE` 1)
    /// and gives the PBKDF2 parameters; then one class with its key wrapped
    /// by the password key (`WRAP` 2).
    fn key_bag(&self) -> Vec<u8> {
        let mut bag = Vec::new();
        tlv(&mut bag, b"TYPE", &1u32.to_be_bytes());
        tlv(&mut bag, b"UUID", &[0xAA; 16]);
        tlv(&mut bag, b"WRAP", &0u32.to_be_bytes());
        tlv(&mut bag, b"DPSL", DPSL);
        tlv(&mut bag, b"DPIC", &DPIC.to_be_bytes());
        tlv(&mut bag, b"SALT", SALT);
        tlv(&mut bag, b"ITER", &ITER.to_be_bytes());
        tlv(&mut bag, b"UUID", &[0xBB; 16]);
        tlv(&mut bag, b"CLAS", &CLASS.to_be_bytes());
        tlv(&mut bag, b"WRAP", &2u32.to_be_bytes());
        tlv(
            &mut bag,
            b"WPKY",
            &aes_key_wrap(&self.password_key, &CLASS_KEY),
        );
        bag
    }

    /// A file's `Manifest.db` metadata: the keyed archive an iPhone writes,
    /// cut down to the fields `crabapple` reads, with the file's own key
    /// wrapped by the class key.
    fn metadata(&self, size: u64, file_key: &[u8; 32]) -> Vec<u8> {
        let mut file = Dictionary::new();
        for key in [
            "LastModified",
            "Flags",
            "GroupID",
            "LastStatusChange",
            "Birth",
            "Mode",
            "InodeNumber",
        ] {
            file.insert(key.into(), Value::Integer(1.into()));
        }
        file.insert("Size".into(), Value::Integer(size.into()));
        file.insert("ProtectionClass".into(), Value::Integer(CLASS.into()));
        file.insert("EncryptionKey".into(), Value::Uid(Uid::new(2)));
        let mut key = Dictionary::new();
        key.insert("NS.data".into(), Value::Data(class_wrapped(file_key)));
        let mut top = Dictionary::new();
        top.insert("root".into(), Value::Uid(Uid::new(1)));
        let mut archive = Dictionary::new();
        archive.insert("$top".into(), Value::Dictionary(top));
        archive.insert(
            "$objects".into(),
            Value::Array(vec![
                Value::String("$null".into()),
                Value::Dictionary(file),
                Value::Dictionary(key),
            ]),
        );
        let mut bytes = Vec::new();
        Value::Dictionary(archive)
            .to_writer_binary(&mut bytes)
            .expect("write the file's metadata");
        bytes
    }
}

/// `Manifest.plist`: whether the backup is encrypted, the device it came
/// from, and for an encrypted backup the key bag and the wrapped
/// `Manifest.db` key.
fn write_manifest_plist(root: &Path, keys: Option<&Keys>) {
    let mut lockdown = Dictionary::new();
    for (key, value) in [
        ("BuildVersion", "21A000"),
        ("DeviceName", "Fixture iPhone"),
        ("ProductType", "iPhone0,0"),
        ("ProductVersion", "17.0"),
        ("SerialNumber", "FIXTURESERIAL"),
        ("UniqueDeviceID", "FIXTUREDEVICEID"),
    ] {
        lockdown.insert(key.into(), Value::String(value.into()));
    }
    let mut manifest = Dictionary::new();
    manifest.insert("IsEncrypted".into(), Value::Boolean(keys.is_some()));
    if let Some(keys) = keys {
        manifest.insert("BackupKeyBag".into(), Value::Data(keys.key_bag()));
        manifest.insert(
            "ManifestKey".into(),
            Value::Data(class_wrapped(&MANIFEST_KEY)),
        );
    }
    manifest.insert("Lockdown".into(), Value::Dictionary(lockdown));
    manifest.insert("Applications".into(), Value::Dictionary(Dictionary::new()));
    Value::Dictionary(manifest)
        .to_file_binary(root.join("Manifest.plist"))
        .expect("write Manifest.plist");
}

/// A key as an encrypted backup stores it: the protection class
/// (little-endian), then the key wrapped with that class's key.
fn class_wrapped(key: &[u8; 32]) -> Vec<u8> {
    let mut out = CLASS.to_le_bytes().to_vec();
    out.extend(aes_key_wrap(&CLASS_KEY, key));
    out
}

/// AES-256 key wrap (RFC 3394) of `key` under `kek`.
fn aes_key_wrap(kek: &[u8; 32], key: &[u8; 32]) -> Vec<u8> {
    let mut out = [0u8; 40];
    KwAes256::new_from_slice(kek)
        .expect("a 32-byte key-encryption key")
        .wrap_key(key, &mut out)
        .expect("wrap the key");
    out.to_vec()
}

/// AES-256-CBC with a zero IV and PKCS#7 padding, the cipher an iPhone
/// backup encrypts files and `Manifest.db` with.
fn aes_cbc_encrypt(bytes: &[u8], key: &[u8; 32]) -> Vec<u8> {
    let mut buffer = vec![0u8; bytes.len() + 16];
    buffer[..bytes.len()].copy_from_slice(bytes);
    let length = cbc::Encryptor::<Aes256>::new(key.into(), &[0u8; 16].into())
        .encrypt_padded::<Pkcs7>(&mut buffer, bytes.len())
        .expect("the buffer holds the padding")
        .len();
    buffer.truncate(length);
    buffer
}

/// Append one key bag entry: a four-letter tag, the value's length as a
/// big-endian `u32`, and the value.
fn tlv(out: &mut Vec<u8>, tag: &[u8; 4], value: &[u8]) {
    out.extend(tag);
    out.extend((value.len() as u32).to_be_bytes());
    out.extend(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ids are the ones every iPhone backup uses for these files, which
    /// `imessage-database` and the reader name as constants.
    #[test]
    fn the_messages_and_contacts_databases_have_the_ids_an_iphone_gives_them() {
        assert_eq!(
            file_id(HOME_DOMAIN, MESSAGES_DB_PATH),
            "3d0d7e5fb2ce288813306e4d4636395e047a3d28"
        );
        assert_eq!(
            file_id(HOME_DOMAIN, CONTACTS_DB_PATH),
            "31bb7ba8914766d4ba40d6dfb6113c8b614be442"
        );
    }

    /// The unencrypted backup stores each file as it is; the encrypted one
    /// stores none of them in the clear.
    #[test]
    fn only_the_unencrypted_backup_holds_its_files_in_the_clear() {
        let plain = tempfile::tempdir().unwrap();
        write_messages_backup(plain.path(), Encryption::None);
        let photo = stored_path(plain.path(), MEDIA_DOMAIN, PHOTO_PATH);
        assert_eq!(fs::read(&photo).unwrap(), PHOTO_BYTES);
        let manifest = Connection::open(plain.path().join("Manifest.db")).unwrap();
        let listed: i64 = manifest
            .query_row("SELECT count(*) FROM Files", [], |row| row.get(0))
            .unwrap();
        assert_eq!(listed, 3);
        assert!(!plain.path().join(".fixture-work").exists());

        let encrypted = tempfile::tempdir().unwrap();
        write_messages_backup(encrypted.path(), Encryption::Password(BACKUP_PASSWORD));
        let photo = fs::read(stored_path(encrypted.path(), MEDIA_DOMAIN, PHOTO_PATH)).unwrap();
        assert_ne!(photo, PHOTO_BYTES);
        assert_eq!(photo.len(), 32, "17 bytes padded to two AES blocks");
        let messages =
            fs::read(stored_path(encrypted.path(), HOME_DOMAIN, MESSAGES_DB_PATH)).unwrap();
        assert!(!messages.starts_with(b"SQLite format 3"));
        let manifest = fs::read(encrypted.path().join("Manifest.db")).unwrap();
        assert!(!manifest.starts_with(b"SQLite format 3"));
    }
}
