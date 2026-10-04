//! Look up contact names from macOS Address Book or iOS Contacts databases.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use imessage_database::{
    error::table::TableError, tables::table::get_connection, util::dirs::home,
};
use rusqlite::{Connection, Result};

use crate::log::emit_log;

/// Default contacts database path inside an iOS backup.
pub(crate) const DEFAULT_PATH_IOS: &str = "31/31bb7ba8914766d4ba40d6dfb6113c8b614be442";

/// Minimum digits required to index a phone number.
const MIN_PHONE_DIGITS: usize = 7;

// MARK: Name
#[derive(Clone, Debug, PartialEq, Eq)]
/// A contact's name, as the contacts database gives it.
pub(crate) struct Name {
    /// First name.
    pub first: String,
    /// Last name.
    pub last: String,
    /// Full name.
    pub full: String,
}

impl Name {
    /// Build a name from optional first and last name fields.
    fn from_opt(first: Option<String>, last: Option<String>) -> Option<Self> {
        // Return None if both are None
        if first.is_none() && last.is_none() {
            return None;
        }

        // Build full name
        let full = format!(
            "{}{}{}",
            first.as_deref().unwrap_or(""),
            if first.is_some() && last.is_some() {
                " "
            } else {
                ""
            },
            last.as_deref().unwrap_or(""),
        );

        Some(Name {
            first: first.unwrap_or_default(),
            last: last.unwrap_or_default(),
            full,
        })
    }

    /// Score name completeness: one point each for first and last name.
    fn score(&self) -> u8 {
        u8::from(!self.first.is_empty()) + u8::from(!self.last.is_empty())
    }
}

// MARK: Index
#[derive(Debug, Default)]
/// Contacts index keyed by normalized phone number or email address.
pub(crate) struct ContactsIndex {
    /// Names keyed by normalized identifier.
    index: HashMap<String, Name>,
}

impl ContactsIndex {
    /// Build a contacts index from one database path or all local macOS sources.
    ///
    /// When `path` is `Some`, only that database is read. When `path` is `None`,
    /// scans macOS Contacts sources under
    /// `~/Library/Application Support/AddressBook/Sources/*/AddressBook-v22.abcddb`.
    ///
    /// Accepts macOS (`AddressBook-v22.abcddb`) and iOS (`AddressBook.sqlitedb`)
    /// databases.
    ///
    /// # Errors
    ///
    /// Returns an error when the database at `path` cannot be opened or
    /// queried, or when the macOS Sources folder exists but cannot be read.
    /// A macOS source that cannot be read is named in the log and left out,
    /// so the other sources' names are kept.
    pub fn build(path: Option<&Path>) -> Result<Self, TableError> {
        if let Some(path) = path {
            let conn = get_connection(path)?;
            if table_exists(&conn, "ABPersonFullTextSearch_content") {
                return Ok(Self::build_from_ios(&conn)?);
            }
            return Ok(Self::build_from_macos(&conn)?);
        }

        let (index, unreadable) = Self::build_from_macos_sources(&macos_sources_dir())?;
        for line in unreadable {
            emit_log(format!(
                "Unable to read a contacts source: {line}\nContinuing without its contact names..."
            ));
        }
        Ok(index)
    }

    /// Build a contacts index from every source under the macOS Contacts
    /// `sources_dir`, with a line naming each source that could not be read.
    ///
    /// A source that cannot be read is left out rather than failing the
    /// index, because the names the other sources give are still right.
    ///
    /// # Errors
    ///
    /// Returns an error when `sources_dir` exists but cannot be read.
    fn build_from_macos_sources(sources_dir: &Path) -> Result<(Self, Vec<String>), TableError> {
        let SourcesScan {
            databases,
            mut unreadable,
        } = find_macos_addressbook_db_paths(sources_dir)?;
        let mut idx: HashMap<String, Name> = HashMap::new();
        for db_path in databases {
            let sub = get_connection(&db_path)
                .and_then(|conn| Self::build_from_macos(&conn).map_err(TableError::from));
            match sub {
                Ok(sub) => {
                    for (k, v) in sub.index {
                        upsert_best(&mut idx, k, &v);
                    }
                }
                Err(e) => unreadable.push(format!("{}: {e}", db_path.display())),
            }
        }
        Ok((Self { index: idx }, unreadable))
    }

    // MARK: macOS
    /// Build a contacts index from a macOS Contacts database.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails.
    fn build_from_macos(conn: &Connection) -> Result<Self> {
        let mut index = HashMap::new();

        let mut stmt = conn.prepare(
            "SELECT r.ZFIRSTNAME, r.ZLASTNAME, p.ZFULLNUMBER, e.ZADDRESSNORMALIZED
             FROM ZABCDRECORD AS r
             LEFT JOIN ZABCDPHONENUMBER AS p ON r.Z_PK = p.ZOWNER
             LEFT JOIN ZABCDEMAILADDRESS AS e ON r.Z_PK = e.ZOWNER",
        )?;

        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name = Name::from_opt(
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            );

            if let Some(name) = name {
                if let Some(email_raw) = row.get::<_, Option<String>>(3)? {
                    // Some macOS rows are like "<addr@dom>"
                    for email in parse_email_list(&email_raw) {
                        upsert_best(&mut index, email, &name);
                    }
                }

                if let Some(phone_raw) = row.get::<_, Option<String>>(2)? {
                    for key in phone_keys(&phone_raw) {
                        upsert_best(&mut index, key, &name);
                    }
                }
            }
        }

        Ok(Self { index })
    }

    // MARK: iOS
    /// Build a contacts index from an iOS backup Contacts database.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails.
    fn build_from_ios(conn: &Connection) -> Result<Self> {
        // iOS backup contacts: ABPersonFullTextSearch_content with columns:
        // c0First (TEXT), c1Last (TEXT), c16Phone (TEXT: space-separated variants), c17Email (TEXT: space-separated)
        let mut index = HashMap::new();

        let mut stmt = conn.prepare(
            "SELECT c0First, c1Last, c16Phone, c17Email
             FROM ABPersonFullTextSearch_content",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name = Name::from_opt(
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            );

            if let Some(name) = name {
                if let Some(phones_blob) = row.get::<_, Option<String>>(2)? {
                    for token in phones_blob.split_whitespace() {
                        for key in phone_keys(token) {
                            upsert_best(&mut index, key, &name);
                        }
                    }
                }

                if let Some(emails_blob) = row.get::<_, Option<String>>(3)? {
                    for email in emails_blob.split_whitespace() {
                        if let Some(norm) = normalize_email(email) {
                            upsert_best(&mut index, norm, &name);
                        }
                    }
                }
            }
        }

        Ok(Self { index })
    }

    /// Look up a contact name by handle string.
    pub fn lookup(&self, id: &str) -> Option<Name> {
        // Look up each space-separated token
        for id_part in id.split_whitespace() {
            if looks_like_email(id_part) {
                if let Some(name) =
                    normalize_email(id_part).and_then(|k| self.index.get(&k).cloned())
                {
                    return Some(name);
                }
                continue;
            }

            for k in phone_keys(id_part) {
                if let Some(n) = self.index.get(&k) {
                    return Some(n.clone());
                }
            }
        }
        None
    }
}

/// Return whether a table or view exists in the database.
fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type IN ('table','view') AND name = ?1 LIMIT 1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

/// Insert a name when it is more complete than the existing value.
fn upsert_best(map: &mut HashMap<String, Name>, key: String, incoming: &Name) {
    match map.get_mut(&key) {
        Some(existing) => {
            if incoming.score() > existing.score() {
                *existing = incoming.clone();
            }
        }
        None => {
            map.insert(key, incoming.clone());
        }
    }
}

// MARK: Email
/// `true` when the identifier looks like an email address.
fn looks_like_email(s: &str) -> bool {
    s.contains('@')
}

/// Normalize an email by trimming, lowercasing, and removing angle brackets.
fn normalize_email(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.trim_start_matches('<').trim_end_matches('>');
    if s.is_empty() {
        return None;
    }
    Some(s.to_lowercase())
}

/// Parse a space-separated email list.
fn parse_email_list(raw: &str) -> Vec<String> {
    if raw.contains(' ') {
        raw.split_whitespace().filter_map(normalize_email).collect()
    } else {
        normalize_email(raw).into_iter().collect()
    }
}

// MARK: Phone
/// Build lookup keys from a raw phone number.
///
/// - If the number contains "urn:", returns an empty vector
/// - If the number has fewer than [`MIN_PHONE_DIGITS`] digits, returns an empty vector
/// - Returns keys with and without '+' prefix
/// - For US numbers starting with +1 and 11 digits, also adds variants without the `+1` country code
fn phone_keys(raw: &str) -> Vec<String> {
    // Skip iMessage business accounts
    if raw.contains("urn:") {
        return vec![];
    }

    // The digits include the country code portion of the number
    let digits = to_phone_digits(raw);

    // Skip numbers that are too short to be valid phone numbers
    // This prevents matching on country codes alone (e.g., "+1" -> "1")
    if digits.len() < MIN_PHONE_DIGITS {
        return vec![];
    }

    let mut keys = vec![digits.clone(), format!("+{digits}")];

    // US numbers may appear with or without the country code.
    if digits.len() == 11 && raw.starts_with("+1") {
        let last_10 = &digits[digits.len() - 10..];
        keys.push(last_10.to_string());
        keys.push(format!("+{last_10}"));
    }

    keys.dedup();
    keys
}

/// Extract digits from a raw phone number string.
/// Keep only ASCII digits from a phone-like string.
fn to_phone_digits(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_digit() {
            out.push(ch);
        }
    }
    out
}

// MARK: macOS Dirs
/// What a scan of the macOS Contacts Sources folder found.
#[derive(Debug, Default, PartialEq, Eq)]
struct SourcesScan {
    /// The AddressBook-v22.abcddb database of each source folder.
    databases: Vec<PathBuf>,
    /// A line naming each entry or source folder that could not be read,
    /// with the error.
    unreadable: Vec<String>,
}

/// Scans a macOS Contacts Sources directory ([`macos_sources_dir`]) for
/// the AddressBook-v22.abcddb database each source folder holds.
///
/// A Mac with no Contacts sources has no Sources folder, so a folder that
/// does not exist is an empty scan.
///
/// # Errors
///
/// Returns an error, with `sources_dir` named, when the folder exists but
/// cannot be read. An entry of it that cannot be read is named in the scan
/// instead (see [`addressbook_db_paths`]).
fn find_macos_addressbook_db_paths(sources_dir: &Path) -> Result<SourcesScan, TableError> {
    let entries = match fs::read_dir(sources_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(SourcesScan::default()),
        Err(e) => {
            return Err(TableError::CannotRead(io::Error::new(
                e.kind(),
                format!("read {}: {e}", sources_dir.display()),
            )));
        }
    };
    Ok(addressbook_db_paths(
        sources_dir,
        entries.map(|entry| entry.map(|e| e.path())),
    ))
}

/// The AddressBook-v22.abcddb database of each source folder among
/// `entries`, the paths of `sources_dir`.
///
/// An entry, a source folder or a database whose details cannot be read is
/// named in [`SourcesScan::unreadable`], with the error. Skipping it without
/// a word would leave that source's contacts unnamed in the export with no
/// message. Only a path that does not exist, such as a source folder with no
/// database or a symbolic link to nothing, is not a source.
fn addressbook_db_paths(
    sources_dir: &Path,
    entries: impl IntoIterator<Item = io::Result<PathBuf>>,
) -> SourcesScan {
    let mut scan = SourcesScan::default();
    for entry in entries {
        let path = match entry {
            Ok(path) => path,
            Err(e) => {
                scan.unreadable
                    .push(format!("read an entry of {}: {e}", sources_dir.display()));
                continue;
            }
        };
        let db_path = path.join("AddressBook-v22.abcddb");
        match (kind_of(&path), kind_of(&db_path)) {
            (Ok(Some(folder)), Ok(Some(db))) if folder.is_dir() && db.is_file() => {
                scan.databases.push(db_path);
            }
            (Err(e), _) => scan
                .unreadable
                .push(format!("read {}: {e}", path.display())),
            (Ok(Some(folder)), Err(e)) if folder.is_dir() => {
                scan.unreadable
                    .push(format!("read {}: {e}", db_path.display()));
            }
            _ => {}
        }
    }
    scan
}

/// The type of what is at `path`, following a symbolic link, or `None` when
/// nothing is there.
///
/// # Errors
///
/// Returns the error for any other failure to read it, such as a folder
/// whose permissions refuse it.
fn kind_of(path: &Path) -> io::Result<Option<fs::FileType>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.file_type())),
        Err(e)
            if e.kind() == io::ErrorKind::NotFound || e.kind() == io::ErrorKind::NotADirectory =>
        {
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// Resolve the standard macOS Contacts Sources directory: `~/Library/Application Support/AddressBook/Sources`
fn macos_sources_dir() -> PathBuf {
    PathBuf::from(&home())
        .join("Library")
        .join("Application Support")
        .join("AddressBook")
        .join("Sources")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::test_support::with_folder_mode;
    use crate::test_support::{fill_ios_address_book, fill_macos_address_book};

    /// A handle is looked up by these keys, so a key the builder does not
    /// produce is a contact the reader never matches — the message arrives
    /// with a bare number instead of a name.
    ///
    /// None of these functions had a test. Each was reported as reachable but
    /// unguarded: `phone_keys` could return an empty vector, `to_phone_digits`
    /// an empty string, `normalize_email` `None`, and every message in an
    /// iMessage import would come through unnamed with the suite green.
    #[test]
    fn a_phone_number_yields_the_keys_a_lookup_might_use() {
        // A US number is stored both ways, because Apple records it either
        // way depending on how the message was addressed.
        let us = phone_keys("+1 (555) 555-0119");
        assert!(us.contains(&"15555550119".to_string()), "{us:?}");
        assert!(us.contains(&"+15555550119".to_string()), "{us:?}");
        assert!(us.contains(&"5555550119".to_string()), "{us:?}");
        assert!(us.contains(&"+5555550119".to_string()), "{us:?}");

        // A non-US number keeps its country code and gains no ten-digit form,
        // because the last ten digits of a UK number are not the number.
        let uk = phone_keys("+44 20 7946 0750");
        assert!(uk.contains(&"442079460750".to_string()), "{uk:?}");
        assert!(uk.contains(&"+442079460750".to_string()), "{uk:?}");
        assert_eq!(uk.len(), 2, "no ten-digit variant for a UK number: {uk:?}");

        // An eleven-digit number is US only when it starts `+1`. A French
        // mobile has eleven digits too, and its last ten would match a US
        // number that isn't this person's. 06 39 98 xx xx is ARCEP's range
        // for fiction, so the number is no one's.
        let fr = phone_keys("+33 6 39 98 12 34");
        assert_eq!(
            fr,
            vec!["33639981234".to_string(), "+33639981234".to_string()]
        );
    }

    /// The Mac keeps one Address Book per account, each in a folder under
    /// `~/Library/Application Support/AddressBook/Sources`. Every folder's
    /// database is found; a folder without one and a stray file are not.
    #[test]
    fn every_mac_address_book_under_the_sources_folder_is_found() {
        assert!(
            macos_sources_dir().ends_with("Library/Application Support/AddressBook/Sources"),
            "{}",
            macos_sources_dir().display()
        );

        let sources = tempfile::tempdir().unwrap();
        for account in ["icloud", "exchange"] {
            let folder = sources.path().join(account);
            fs::create_dir(&folder).unwrap();
            fill_macos_address_book(
                &Connection::open(folder.join("AddressBook-v22.abcddb")).unwrap(),
            );
        }
        fs::create_dir(sources.path().join("empty")).unwrap();
        fs::write(sources.path().join("AddressBook-v22.abcddb"), b"").unwrap();

        let scan = find_macos_addressbook_db_paths(sources.path()).unwrap();
        assert_eq!(scan.unreadable, Vec::<String>::new());
        let mut found = scan.databases;
        found.sort();
        assert_eq!(
            found,
            vec![
                sources.path().join("exchange/AddressBook-v22.abcddb"),
                sources.path().join("icloud/AddressBook-v22.abcddb"),
            ]
        );
        assert_eq!(
            find_macos_addressbook_db_paths(&sources.path().join("missing")).unwrap(),
            SourcesScan::default()
        );
    }

    /// An entry of the Sources folder that cannot be read is named, with
    /// the folder and the error, and the sources that read are kept, rather
    /// than leaving that source's contacts unnamed with no message (#1563).
    #[test]
    fn an_entry_of_the_sources_folder_that_cannot_be_read_is_named_and_the_rest_kept() {
        let sources = tempfile::tempdir().unwrap();
        let icloud = sources.path().join("icloud");
        fs::create_dir(&icloud).unwrap();
        fs::write(icloud.join("AddressBook-v22.abcddb"), b"").unwrap();
        let entries = vec![
            Err(io::Error::other("stale file handle")),
            Ok(icloud.clone()),
        ];

        let scan = addressbook_db_paths(sources.path(), entries);

        assert_eq!(scan.databases, vec![icloud.join("AddressBook-v22.abcddb")]);
        assert_eq!(scan.unreadable.len(), 1, "{:?}", scan.unreadable);
        let line = &scan.unreadable[0];
        assert!(
            line.contains(&sources.path().display().to_string()),
            "{line}"
        );
        assert!(line.contains("stale file handle"), "{line}");
    }

    /// A Contacts account folder that cannot be read is named, and the
    /// other accounts' names are still in the index (#1563).
    #[cfg(unix)]
    #[test]
    fn an_account_folder_that_cannot_be_read_is_named_and_the_others_names_kept() {
        let sources = tempfile::tempdir().unwrap();
        for account in ["icloud", "exchange"] {
            let folder = sources.path().join(account);
            fs::create_dir(&folder).unwrap();
            fill_macos_address_book(
                &Connection::open(folder.join("AddressBook-v22.abcddb")).unwrap(),
            );
        }
        let exchange = sources.path().join("exchange");

        let Some(built) = with_folder_mode(&exchange, 0o000, || {
            ContactsIndex::build_from_macos_sources(sources.path())
        }) else {
            return;
        };

        let (index, unreadable) = built.unwrap();
        assert_eq!(
            index.lookup("+15555550107").map(|n| n.full).as_deref(),
            Some("Sam Example")
        );
        assert_eq!(unreadable.len(), 1, "{unreadable:?}");
        assert!(
            unreadable[0].contains(&exchange.display().to_string()),
            "{unreadable:?}"
        );
    }

    /// An account whose Contacts database file is not SQLite is named, and
    /// the other accounts' names are still in the index.
    #[test]
    fn a_contacts_database_that_cannot_be_queried_is_named_and_the_others_names_kept() {
        let sources = tempfile::tempdir().unwrap();
        let icloud = sources.path().join("icloud");
        fs::create_dir(&icloud).unwrap();
        fill_macos_address_book(&Connection::open(icloud.join("AddressBook-v22.abcddb")).unwrap());
        let broken = sources.path().join("broken");
        fs::create_dir(&broken).unwrap();
        fs::write(
            broken.join("AddressBook-v22.abcddb"),
            b"this is not a SQLite database, only some text long enough to be read as a header",
        )
        .unwrap();

        let (index, unreadable) = ContactsIndex::build_from_macos_sources(sources.path()).unwrap();

        assert!(index.lookup("+15555550107").is_some());
        assert_eq!(unreadable.len(), 1, "{unreadable:?}");
        assert!(
            unreadable[0].contains(&broken.display().to_string()),
            "{unreadable:?}"
        );
    }

    /// A Sources folder that exists but cannot be read fails the scan with
    /// the folder named, rather than reading as a Mac with no contacts.
    #[cfg(unix)]
    #[test]
    fn a_sources_folder_that_cannot_be_read_fails_the_scan_and_names_it() {
        let sources = tempfile::tempdir().unwrap();
        fs::create_dir(sources.path().join("icloud")).unwrap();

        let Some(result) = with_folder_mode(sources.path(), 0o000, || {
            find_macos_addressbook_db_paths(sources.path())
        }) else {
            return;
        };

        let error = result.unwrap_err().to_string();
        assert!(
            error.contains(&sources.path().display().to_string()),
            "{error}"
        );
    }

    #[test]
    fn a_handle_that_is_not_a_phone_number_yields_no_keys() {
        // An iMessage business account is not a person.
        assert!(phone_keys("urn:biz:apple").is_empty());
        // Too few digits to be a number: a country code on its own must not
        // become a key that matches every number in that country.
        assert!(phone_keys("+1").is_empty());
        assert!(phone_keys("123").is_empty());
        assert!(phone_keys("").is_empty());
    }

    #[test]
    fn phone_digits_are_the_digits_and_nothing_else() {
        assert_eq!(to_phone_digits("+1 (555) 555-0119"), "15555550119");
        assert_eq!(to_phone_digits("555.555.0119"), "5555550119");
        assert_eq!(to_phone_digits("no digits here"), "");
        assert_eq!(to_phone_digits(""), "");
    }

    #[test]
    fn an_email_is_lower_cased_and_stripped_of_its_brackets() {
        assert_eq!(
            normalize_email("  <Alice@Example.COM>  "),
            Some("alice@example.com".to_string())
        );
        assert_eq!(
            normalize_email("alice@example.com"),
            Some("alice@example.com".to_string())
        );
        // Nothing but whitespace or brackets is not an address.
        assert_eq!(normalize_email("   "), None);
        assert_eq!(normalize_email("<>"), None);
        assert_eq!(normalize_email(""), None);
    }

    #[test]
    fn an_email_list_splits_on_spaces_and_a_single_address_does_not() {
        assert_eq!(
            parse_email_list("Alice@Example.com  bob@example.com"),
            vec![
                "alice@example.com".to_string(),
                "bob@example.com".to_string()
            ]
        );
        assert_eq!(
            parse_email_list("alice@example.com"),
            vec!["alice@example.com".to_string()]
        );
        assert!(parse_email_list("   ").is_empty());
    }

    #[test]
    fn an_identifier_is_an_email_when_it_has_an_at_sign() {
        assert!(looks_like_email("alice@example.com"));
        assert!(!looks_like_email("+15555550119"));
        assert!(!looks_like_email(""));
    }

    /// When two address-book rows name the same handle, the fuller name wins.
    /// `upsert_best` is what decides, and replacing it with "keep the first"
    /// or "keep the last" means a contact is named "Sam" or "" when the book
    /// has "Sam Example".
    #[test]
    fn the_fuller_name_wins_when_two_rows_name_one_handle() {
        let first_only = Name::from_opt(Some("Sam".into()), None).expect("a first name");
        let full = Name::from_opt(Some("Sam".into()), Some("Example".into())).expect("a full name");

        // Fuller arriving second replaces the sparser one.
        let mut map = HashMap::new();
        upsert_best(&mut map, "key".into(), &first_only);
        upsert_best(&mut map, "key".into(), &full);
        assert_eq!(map["key"].last, "Example");

        // Fuller arriving first is not replaced by the sparser one.
        let mut map = HashMap::new();
        upsert_best(&mut map, "key".into(), &full);
        upsert_best(&mut map, "key".into(), &first_only);
        assert_eq!(
            map["key"].last, "Example",
            "a sparser row must not overwrite a fuller one"
        );
    }

    #[test]
    fn a_name_is_built_from_whichever_parts_exist() {
        let both = Name::from_opt(Some("Sam".into()), Some("Example".into())).unwrap();
        assert_eq!((both.full.as_str(), both.score()), ("Sam Example", 2));

        let last_only = Name::from_opt(None, Some("Example".into())).unwrap();
        assert_eq!((last_only.full.as_str(), last_only.score()), ("Example", 1));

        assert_eq!(Name::from_opt(None, None), None);
    }

    fn macos_address_book() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        fill_macos_address_book(&conn);
        conn
    }

    fn ios_address_book() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        fill_ios_address_book(&conn);
        conn
    }

    #[test]
    fn a_table_exists_by_name_or_not_at_all() {
        let conn = ios_address_book();
        assert!(table_exists(&conn, "ABPersonFullTextSearch_content"));
        assert!(!table_exists(&conn, "ZABCDRECORD"));
        assert!(!table_exists(
            &macos_address_book(),
            "ABPersonFullTextSearch_content"
        ));
    }

    /// A macOS book joins a record to its numbers and addresses; every
    /// handle form of each reaches the same name, and a row with no name
    /// indexes nothing even though it has a number.
    #[test]
    fn a_macos_address_book_is_indexed_by_phone_and_email() {
        let index = ContactsIndex::build_from_macos(&macos_address_book()).unwrap();

        let sam = index.lookup("+15555550107").expect("Sam by full number");
        assert_eq!(sam.full, "Sam Example");
        assert_eq!(
            index.lookup("5555550107").map(|n| n.full),
            Some("Sam Example".to_string()),
            "the ten-digit form of a US number"
        );
        assert_eq!(
            index.lookup("sam@example.com").map(|n| n.full),
            Some("Sam Example".to_string()),
            "brackets stripped and lower-cased"
        );
        assert_eq!(
            index.lookup("friend@example.com").map(|n| n.full),
            Some("Robin".to_string())
        );
        assert_eq!(
            index.lookup("+15555550179"),
            None,
            "a record with no name is not a contact"
        );
        assert_eq!(index.lookup("nobody@example.com"), None);
    }

    /// The iOS table carries every form of a number in one column; each form
    /// is indexed, and each address in the email column too.
    #[test]
    fn an_ios_address_book_is_indexed_by_every_phone_and_email_token() {
        let index = ContactsIndex::build_from_ios(&ios_address_book()).unwrap();

        for handle in [
            "+15555550107",
            "5555550107",
            "sam@example.com",
            "sam@work.example",
        ] {
            assert_eq!(
                index.lookup(handle).map(|n| n.full),
                Some("Sam Example".to_string()),
                "{handle}"
            );
        }
        assert_eq!(
            index.lookup("friend@example.com").map(|n| n.full),
            Some("Robin".to_string())
        );
        assert_eq!(index.lookup("+15555550179"), None);
    }

    /// `build` picks the query by the table it finds, so one path serves a
    /// macOS book and an iOS one.
    #[test]
    fn build_tells_the_two_databases_apart_by_their_tables() {
        let dir = tempfile::tempdir().unwrap();

        let ios_path = dir.path().join("AddressBook.sqlitedb");
        fill_ios_address_book(&Connection::open(&ios_path).unwrap());
        let index = ContactsIndex::build(Some(&ios_path)).unwrap();
        assert_eq!(
            index.lookup("sam@work.example").map(|n| n.full),
            Some("Sam Example".to_string())
        );

        let macos_path = dir.path().join("AddressBook-v22.abcddb");
        fill_macos_address_book(&Connection::open(&macos_path).unwrap());
        let index = ContactsIndex::build(Some(&macos_path)).unwrap();
        assert_eq!(
            index.lookup("friend@example.com").map(|n| n.full),
            Some("Robin".to_string())
        );
    }
}
