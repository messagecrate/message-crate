//! The address book: Message Crate's own CSV of contacts and their
//! identities, one row per identity.
//!
//! [`export_csv`] writes it and [`load`] reads it back, and [`from_vcard`]
//! writes one from a vCard file for a person to edit. The rules are in
//! `docs/architecture/contacts-identities-and-messages.md`, the five starting
//! at "The address book is a file for editing contacts, not a source of
//! them": a load is Append or Edit, touches only the contacts in the file, is
//! strict, refuses whole, and moves an identity only from a contact the load
//! may change.
//!
//! A load reads the account's contacts, identities and Contact Groups once,
//! checks every row against that picture, and writes only when no row was
//! refused. Everything runs in one transaction.

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use ir::{IdentityService, IdentityType};
use message_crate_csv::{read_cell, write_guarded_record};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::db::contacts::{self, Origin};
use crate::db::trash::not_trashed_contact;
use crate::db::{WriteTx, begin_write};
use crate::db::{handles, named_membership};

/// The columns of the file, in the order Export writes them.
pub const COLUMNS: [&str; 5] = [
    "contact_id",
    "display_name",
    "service",
    "identity",
    "groups",
];

/// What a `service` cell says: the service an identity is on, in the words
/// the screen where an identity is added offers (`OFFERED_SERVICE_OPTIONS`
/// in `web/src/lib/offeredService.ts`). The file states no type: the service
/// decides which types are valid and the address's shape picks among them
/// (`docs/architecture/contacts-identities-and-messages.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileService {
    /// `Text Message`: a phone number, an email address, or any other name,
    /// on the Text Message service.
    TextMessage,
    /// `Email`: an email address on the Text Message service, where
    /// iMessage reaches it.
    Email,
    /// `WhatsApp`: a phone number or one of WhatsApp's own ids.
    Whatsapp,
}

impl FileService {
    /// Every word, in the order a refusal lists them.
    const ALL: [Self; 3] = [Self::TextMessage, Self::Email, Self::Whatsapp];

    /// The word the file writes.
    fn word(self) -> &'static str {
        match self {
            Self::TextMessage => "Text Message",
            Self::Email => "Email",
            Self::Whatsapp => "WhatsApp",
        }
    }

    /// The word `text` names, ignoring case, or `None` for any other text.
    fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|word| word.word().eq_ignore_ascii_case(text))
    }

    /// The service the identity is stored on.
    fn service(self) -> IdentityService {
        match self {
            Self::TextMessage | Self::Email => IdentityService::Phone,
            Self::Whatsapp => IdentityService::Whatsapp,
        }
    }

    /// The word Export writes for an identity stored with `service` and
    /// `handle_type`: `Email` for an email address, which can only be on
    /// Text Message, and the service's own word for anything else.
    fn of(service: IdentityService, handle_type: IdentityType) -> Self {
        match (service, handle_type) {
            (IdentityService::Whatsapp, _) => Self::Whatsapp,
            (IdentityService::Phone, IdentityType::Email) => Self::Email,
            (IdentityService::Phone, _) => Self::TextMessage,
        }
    }

    /// The types an identity the account already holds may have when a row
    /// writes it exactly as the database keys it. An `Email` row names an
    /// email address only.
    fn held_types(self) -> &'static [IdentityType] {
        match self {
            Self::Email => &[IdentityType::Email],
            Self::TextMessage => &[
                IdentityType::Phone,
                IdentityType::Email,
                IdentityType::Other,
            ],
            Self::Whatsapp => &[IdentityType::Phone, IdentityType::Other],
        }
    }
}

/// What separates Contact Group names in the `groups` column. A Contact
/// Group's name may not hold it (`named_membership::group_spec`), so the cell
/// needs no escaping and an export loads back as the groups it was written from.
pub(crate) const GROUP_SEPARATOR: char = ';';

/// How a load applies the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoadMode {
    /// Create the contacts the file names, rename the ones it holds, and add
    /// the identities and Contact Group memberships it lists. Nothing is
    /// removed.
    #[default]
    Append,
    /// Append, and then make each contact in the file hold exactly the
    /// identities and memberships its rows list.
    Edit,
}

/// What a load changed. `POST /v1/contacts` answers it as
/// `CreateContactsResponse`, whose fields say what each count means.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LoadCounts {
    /// Contacts created.
    pub contacts_created: u64,
    /// Contacts updated.
    pub contacts_updated: u64,
    /// Contacts deleted.
    pub contacts_deleted: u64,
    /// Identities added.
    pub identities_added: u64,
    /// Identities moved.
    pub identities_moved: u64,
    /// Identities removed.
    pub identities_removed: u64,
    /// Contact Groups created.
    pub groups_created: u64,
    /// How each phone number written without `+` was read.
    pub notes: Vec<String>,
}

impl LoadCounts {
    /// The contacts the load created or updated.
    #[must_use]
    pub fn contacts_changed(&self) -> u64 {
        self.contacts_created + self.contacts_updated
    }
}

/// Why a load did not happen.
#[derive(Debug)]
pub enum LoadError {
    /// The file broke a rule. Each sentence names a row and its reason, and
    /// nothing was written.
    Refused(Vec<String>),
    /// Something failed that changing the file would not help.
    Failed(anyhow::Error),
}

impl From<anyhow::Error> for LoadError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(reasons) => {
                write!(f, "the address book was refused: {}", reasons.join("; "))
            }
            Self::Failed(cause) => write!(f, "{cause:#}"),
        }
    }
}

/// The key of one identity: the three columns `handles` is unique on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct IdentityKey {
    service: IdentityService,
    handle_type: &'static str,
    normalized: String,
}

/// One data row of the file, fields trimmed.
#[derive(Debug)]
struct FileRow {
    /// The row as a spreadsheet numbers it: the header is row 1.
    number: usize,
    contact_id: String,
    display_name: String,
    service: String,
    identity: String,
    groups: String,
}

/// Which contact a group of rows speaks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Target {
    /// A contact the account holds, by its id.
    Known(i64),
    /// A contact the load creates.
    New,
}

impl Target {
    /// The id of the contact the account holds, or `None` for a new one.
    fn known_id(self) -> Option<i64> {
        match self {
            Self::Known(id) => Some(id),
            Self::New => None,
        }
    }
}

/// One identity a file contact lists.
#[derive(Debug)]
struct FileIdentity {
    row: usize,
    key: IdentityKey,
    /// The identity as the file wrote it, kept as the `raw` of a new row.
    written: String,
    /// What the load says about how it read this row, for
    /// [`LoadCounts::notes`].
    note: Option<String>,
}

/// The rows of one contact, gathered.
#[derive(Debug)]
struct FileContact {
    target: Target,
    /// The `contact_id` text, for a refusal to quote.
    id_text: String,
    first_row: usize,
    /// The name the rows agree on, and the row that first gave it.
    name: Option<(String, usize)>,
    /// The Contact Group names the rows agree on, and the row that first
    /// gave them. `None` when every row left the column blank.
    groups: Option<(Vec<String>, usize)>,
    identities: Vec<FileIdentity>,
}

impl FileContact {
    /// How a refusal names this contact.
    fn describe(&self) -> String {
        let name = match &self.name {
            Some((name, _)) => format!("\"{name}\""),
            None => "the contact with no name".to_string(),
        };
        match self.target {
            Target::Known(id) => format!("{name} (contact {id})"),
            Target::New if self.id_text.is_empty() => {
                format!("{name} (a new contact, row {})", self.first_row)
            }
            Target::New => format!("{name} (a new contact, contact_id {})", self.id_text),
        }
    }
}

/// What the account holds when the load starts.
#[derive(Debug, Default)]
struct Snapshot {
    /// Live contacts: id to trimmed name.
    contacts: HashMap<i64, String>,
    /// Trashed contacts: id to trimmed name. A trashed contact is not in
    /// Contacts, so the file cannot speak for it, but it can hold an identity.
    trashed: HashMap<i64, String>,
    /// Every identity: its row id and the contact holding it, if any.
    handles: HashMap<IdentityKey, (i64, Option<i64>)>,
    /// Contact Groups by lower-cased name.
    groups: HashMap<String, i64>,
    /// Which groups each contact is a member of.
    memberships: HashMap<i64, HashSet<i64>>,
}

impl Snapshot {
    /// Read the account's contacts, identities and Contact Groups.
    async fn read(conn: &mut SqliteConnection, account_id: i64) -> Result<Self> {
        #[derive(sqlx::FromRow)]
        struct ContactRow {
            id: i64,
            name: String,
            trashed: i64,
        }
        #[derive(sqlx::FromRow)]
        struct IdentityRow {
            id: i64,
            service: String,
            handle_type: String,
            normalized: String,
            holder: Option<i64>,
        }
        #[derive(sqlx::FromRow)]
        struct ContactGroupRow {
            id: i64,
            name: String,
        }
        #[derive(sqlx::FromRow)]
        struct MembershipRow {
            contact_id: i64,
            group_id: i64,
        }
        let mut snapshot = Self::default();
        let rows = sqlx::query_as::<_, ContactRow>(sqlx::AssertSqlSafe(format!(
            "SELECT ct.id, ct.preferred_name AS name, NOT ({not_trashed}) AS trashed
             FROM contacts ct WHERE ct.account_id = $1",
            not_trashed = not_trashed_contact("ct"),
        )))
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await
        .context("read the account's contacts")?;
        for ContactRow { id, name, trashed } in rows {
            if trashed != 0 {
                snapshot.trashed.insert(id, name);
            } else {
                snapshot.contacts.insert(id, name);
            }
        }

        let rows = sqlx::query_as::<_, IdentityRow>(
            "SELECT h.id, h.service, h.handle_type, h.normalized, ch.contact_id AS holder
             FROM handles h
             LEFT JOIN contact_handles ch
               ON ch.account_id = h.account_id AND ch.handle_id = h.id
             WHERE h.account_id = $1",
        )
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await
        .context("read the account's identities and their contacts")?;
        for IdentityRow {
            id,
            service,
            handle_type,
            normalized,
            holder,
        } in rows
        {
            let (Some(service), Some(handle_type)) =
                (parse_service(&service), parse_handle_type(&handle_type))
            else {
                continue;
            };
            snapshot.handles.insert(
                IdentityKey {
                    service,
                    handle_type: handle_type.as_str(),
                    normalized,
                },
                (id, holder),
            );
        }

        let rows = sqlx::query_as::<_, ContactGroupRow>(
            "SELECT id, name FROM contact_groups WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await
        .context("read the account's Contact Groups")?;
        for ContactGroupRow { id, name } in rows {
            snapshot.groups.insert(name.to_lowercase(), id);
        }

        let rows = sqlx::query_as::<_, MembershipRow>(
            "SELECT m.contact_id, m.group_id
             FROM contact_group_members m
             JOIN contact_groups g ON g.id = m.group_id
             WHERE g.account_id = $1",
        )
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await
        .context("read the account's Contact Group memberships")?;
        for MembershipRow {
            contact_id,
            group_id,
        } in rows
        {
            snapshot
                .memberships
                .entry(contact_id)
                .or_default()
                .insert(group_id);
        }
        Ok(snapshot)
    }

    /// The name of a contact, live or trashed.
    fn name_of(&self, contact_id: i64) -> &str {
        self.contacts
            .get(&contact_id)
            .or_else(|| self.trashed.get(&contact_id))
            .map_or("", String::as_str)
    }

    /// How a sentence names a contact the account holds.
    fn describe(&self, contact_id: i64) -> String {
        match self.name_of(contact_id) {
            "" => format!("the contact with no name (contact {contact_id})"),
            name => format!("\"{name}\" (contact {contact_id})"),
        }
    }

    /// Whether a load may take an identity from `holder` without the file
    /// naming it: a contact with no name, or one the file speaks for
    /// (`in_file`).
    fn may_take_from(&self, holder: i64, in_file: &HashSet<i64>) -> bool {
        self.name_of(holder).is_empty() || in_file.contains(&holder)
    }

    /// Whether `contact_id` holds the identity `key`.
    fn holds(&self, contact_id: i64, key: &IdentityKey) -> bool {
        self.handles
            .get(key)
            .is_some_and(|&(_, holder)| holder == Some(contact_id))
    }
}

/// The domains of the WhatsApp ids an import stores as `other`: a person
/// WhatsApp knows by no number (`…@lid`), a group (`…@g.us`), the Status
/// chat (`status@broadcast`) and a channel (`…@newsletter`). A phone id
/// (`…@s.whatsapp.net`) is not one: the WhatsApp exporter writes it as the
/// `+` number (`whatsapp-exporter/src/jid.rs`), so the file writes the number.
const WHATSAPP_ID_DOMAINS: [&str; 4] = ["lid", "g.us", "broadcast", "newsletter"];

/// Whether `address` is one of WhatsApp's own ids rather than an email
/// address: an `@` followed by one of [`WHATSAPP_ID_DOMAINS`], in lower case
/// as WhatsApp writes it. `…@LID` is not one, because a new identity is keyed
/// as written and would match no id an import stored.
fn is_whatsapp_id(address: &str) -> bool {
    address
        .rsplit_once('@')
        .is_some_and(|(_, domain)| WHATSAPP_ID_DOMAINS.contains(&domain))
}

/// The `service` value the `handles` table stores, or `None` for any other
/// text. Unlike [`IdentityService::parse`], which reads every unknown word as
/// the phone platform, a row holding any other word is skipped.
fn parse_service(text: &str) -> Option<IdentityService> {
    [IdentityService::Phone, IdentityService::Whatsapp]
        .into_iter()
        .find(|s| s.as_str().eq_ignore_ascii_case(text))
}

/// The `handle_type` value the `handles` table stores, or `None` for any
/// other text.
fn parse_handle_type(text: &str) -> Option<IdentityType> {
    [
        IdentityType::Phone,
        IdentityType::Email,
        IdentityType::Username,
        IdentityType::Other,
    ]
    .into_iter()
    .find(|t| t.as_str().eq_ignore_ascii_case(text))
}

/// Read the CSV into rows. A row whose every field is blank is skipped, the
/// way a spreadsheet's trailing empty rows are. A row with fewer fields than
/// the header reads its missing trailing cells as blank, since a spreadsheet
/// can drop empty cells at the end of a row and no column moves. A row with
/// more fields than the header is an error: its cells cannot be matched to
/// the columns, most often because a cell holding a comma lacks quotes.
fn read_rows(csv_text: &str) -> Result<Vec<FileRow>, Vec<String>> {
    let text = csv_text.strip_prefix('\u{feff}').unwrap_or(csv_text);
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| vec![format!("the file is not CSV: {e}")])?
        .clone();
    let mut index = [0usize; 5];
    let mut missing = Vec::new();
    for (slot, column) in COLUMNS.iter().enumerate() {
        match headers.iter().position(|h| h.eq_ignore_ascii_case(column)) {
            Some(at) => index[slot] = at,
            None => missing.push(*column),
        }
    }
    if !missing.is_empty() {
        return Err(vec![format!(
            "row 1: the header is missing {}; the columns are {}",
            missing.join(", "),
            COLUMNS.join(", ")
        )]);
    }

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (at, record) in reader.records().enumerate() {
        let number = at + 2;
        let record = match record {
            Ok(record) => record,
            Err(e) => {
                errors.push(format!("row {number}: not CSV: {e}"));
                continue;
            }
        };
        if record.len() > headers.len() && !record.iter().all(str::is_empty) {
            errors.push(format!(
                "row {number}: has {} fields where the header has {}; \
                 put a cell that holds a comma in double quotes",
                record.len(),
                headers.len()
            ));
            continue;
        }
        let field =
            |slot: usize| read_cell(record.get(index[slot]).unwrap_or("").trim()).to_string();
        let row = FileRow {
            number,
            contact_id: field(0),
            display_name: field(1),
            service: field(2),
            identity: field(3),
            groups: field(4),
        };
        let blank = row.contact_id.is_empty()
            && row.display_name.is_empty()
            && row.service.is_empty()
            && row.identity.is_empty()
            && row.groups.is_empty();
        if !blank {
            rows.push(row);
        }
    }
    if errors.is_empty() {
        Ok(rows)
    } else {
        Err(errors)
    }
}

/// The identity a row lists, keyed the way the `handles` table keys it.
/// `Ok(None)` for a row that lists no identity: one that leaves `service`
/// and `identity` both blank, which is how a contact with no identity is
/// written.
///
/// The row states no type. An identity written exactly as the database
/// keys it on the row's service is that identity, whatever its type, so a
/// file Export wrote loads back as it was. Any other is typed by its shape
/// ([`handles::handle_type_of`]) among the types its service carries, as an
/// identity added on the screen is: one of WhatsApp's own ids
/// ([`is_whatsapp_id`]) is `other` on WhatsApp. A shape the service cannot
/// carry, such as an email address on WhatsApp, is refused.
///
/// `contact` is the contact the row names, when the account holds it. A
/// phone number written without `+` that no key matches as written is read
/// as `+` and its digits when that contact holds that key, because a
/// spreadsheet that opens the file can save `+6555550100` as the number
/// `6555550100`. Other contacts' identities are not looked at, so a dropped
/// `+` never attaches another person's number to this one.
fn row_identity(
    row: &FileRow,
    contact: Option<i64>,
    snapshot: &Snapshot,
) -> Result<Option<FileIdentity>, String> {
    let n = row.number;
    if row.service.is_empty() && row.identity.is_empty() {
        return Ok(None);
    }
    let Some(file_service) = FileService::parse(&row.service) else {
        let words: Vec<&str> = FileService::ALL.iter().map(|w| w.word()).collect();
        return Err(format!(
            "row {n}: service \"{}\" is not one Message Crate offers; use {}",
            row.service,
            words.join(", ")
        ));
    };
    if row.identity.is_empty() {
        return Err(format!("row {n}: identity is blank"));
    }
    let service = file_service.service();
    let keyed_as = |handle_type: IdentityType, normalized: String| IdentityKey {
        service,
        handle_type: handle_type.as_str(),
        normalized,
    };
    // An identity written exactly as the database keys it is one Export
    // wrote, so it is accepted as it stands, with the type it was stored
    // with: a file loaded straight back must never be refused over a key an
    // import stored. The shape's own type is tried first.
    let shape = handles::handle_type_of(&row.identity);
    let is_held = |t: IdentityType| {
        snapshot
            .handles
            .contains_key(&keyed_as(t, row.identity.clone()))
    };
    let candidates = file_service.held_types();
    let held = if candidates.contains(&shape) && is_held(shape) {
        Some(shape)
    } else {
        candidates.iter().copied().find(|&t| is_held(t))
    };
    let handle_type = match (held, file_service) {
        (Some(held), _) => held,
        (None, FileService::Email) if shape != IdentityType::Email => {
            return Err(format!(
                "row {n}: \"{}\" is not an email address: it needs one @ with text on both sides",
                row.identity
            ));
        }
        // WhatsApp's own ids ([`WHATSAPP_ID_DOMAINS`]) are `other` there, as
        // an import stores them, and are never an email address. Any other
        // `@` address on WhatsApp is an email address, which WhatsApp cannot
        // carry, and is refused below, as the decision on #916 asks.
        (None, FileService::Whatsapp) if is_whatsapp_id(&row.identity) => IdentityType::Other,
        (None, _) => shape,
    };
    if let Err(refusal) = handles::check_service_carries(&row.identity, service, handle_type) {
        return Err(format!("row {n}: {refusal}"));
    }
    let key = |normalized: String| keyed_as(handle_type, normalized);
    let mut note = None;
    let normalized = if held.is_some() {
        row.identity.clone()
    } else {
        match handle_type {
            IdentityType::Phone => {
                if phone::sanitize_phone_shaped(&row.identity).is_none() {
                    return Err(format!(
                        "row {n}: \"{}\" is not a phone number Message Crate can key: \
                         it needs 4 to 15 digits and nothing but digits, spaces and + - ( ) .",
                        row.identity
                    ));
                }
                let keyed = phone::normalize_typed_handle(&row.identity, IdentityType::Phone).0;
                if row.identity.contains('+') {
                    keyed
                } else {
                    let digits: String =
                        row.identity.chars().filter(char::is_ascii_digit).collect();
                    let with_plus = format!("+{digits}");
                    let holder = contact.filter(|&id| {
                        with_plus != keyed && snapshot.holds(id, &key(with_plus.clone()))
                    });
                    if let Some(id) = holder {
                        if snapshot.holds(id, &key(keyed.clone())) {
                            return Err(format!(
                                "row {n}: {} has no +, and {} holds both {with_plus} and {keyed}; \
                                 write the number with its + to say which",
                                row.identity,
                                snapshot.describe(id)
                            ));
                        }
                        note = Some(format!(
                            "row {n}: {} has no +, so it was read as {with_plus}, which {} holds",
                            row.identity,
                            snapshot.describe(id)
                        ));
                        with_plus
                    } else {
                        if !snapshot.handles.contains_key(&key(keyed.clone())) {
                            note = Some(format!(
                                "row {n}: {} has no +, so it became the new identity {keyed}",
                                row.identity
                            ));
                        }
                        keyed
                    }
                }
            }
            IdentityType::Email => {
                let lowered = row.identity.to_lowercase();
                let mut parts = lowered.split('@');
                let well_formed = matches!(
                    (parts.next(), parts.next(), parts.next()),
                    (Some(local), Some(domain), None) if !local.is_empty() && !domain.is_empty()
                ) && !lowered.chars().any(char::is_whitespace);
                if !well_formed {
                    return Err(format!(
                        "row {n}: \"{}\" is not an email address: it needs one @ with text on both sides",
                        row.identity
                    ));
                }
                lowered
            }
            IdentityType::Username | IdentityType::Other => row.identity.clone(),
        }
    };
    Ok(Some(FileIdentity {
        row: n,
        key: key(normalized),
        written: row.identity.clone(),
        note,
    }))
}

/// The Contact Group names one `groups` cell lists, in the order written,
/// each once. `Err` names the first one the product would not let a person
/// create.
fn row_groups(row: &FileRow) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = Vec::new();
    for name in row.groups.split(GROUP_SEPARATOR).map(str::trim) {
        if name.is_empty() {
            continue;
        }
        let name = named_membership::check_name(named_membership::group_spec(), name)
            .map_err(|reason| format!("row {}: Contact Group \"{name}\": {reason}", row.number))?;
        if !names
            .iter()
            .any(|n| n.to_lowercase() == name.to_lowercase())
        {
            names.push(name);
        }
    }
    Ok(names)
}

/// The same set of group names, whatever their order or case.
fn same_groups(a: &[String], b: &[String]) -> bool {
    let fold =
        |names: &[String]| -> BTreeSet<String> { names.iter().map(|n| n.to_lowercase()).collect() };
    fold(a) == fold(b)
}

/// How rows are gathered into contacts before they are numbered.
#[derive(Debug, PartialEq, Eq, Hash)]
enum GroupingKey {
    Known(i64),
    Text(String),
    /// A blank `contact_id`: every such row is a contact of its own.
    Blank(usize),
}

/// The contacts [`plan`] gathers as it reads the rows.
#[derive(Debug, Default)]
struct Gathering {
    file: Vec<FileContact>,
    /// Which contact of `file` the rows of each key gather into.
    by_key: HashMap<GroupingKey, usize>,
    /// Where each identity was first listed: the contact and the row.
    listed: HashMap<IdentityKey, (usize, usize)>,
}

impl Gathering {
    /// Where in `file` the contact `row` speaks for is, gathered the first
    /// time a row names it.
    fn contact_of(&mut self, row: &FileRow, snapshot: &Snapshot) -> usize {
        let known = row
            .contact_id
            .parse::<i64>()
            .ok()
            .filter(|id| snapshot.contacts.contains_key(id));
        let grouping = match known {
            Some(id) => GroupingKey::Known(id),
            None if row.contact_id.is_empty() => GroupingKey::Blank(row.number),
            None => GroupingKey::Text(row.contact_id.clone()),
        };
        let file = &mut self.file;
        *self.by_key.entry(grouping).or_insert_with(|| {
            file.push(FileContact {
                target: known.map_or(Target::New, Target::Known),
                id_text: row.contact_id.clone(),
                first_row: row.number,
                name: None,
                groups: None,
                identities: Vec::new(),
            });
            file.len() - 1
        })
    }

    /// Add the identity `row` lists to the contact at `at` in `file`. The
    /// row is refused when the identity is not one Message Crate can store,
    /// or when another contact of the file listed it first.
    fn list_identity(
        &mut self,
        at: usize,
        row: &FileRow,
        snapshot: &Snapshot,
        errors: &mut Vec<String>,
    ) {
        let contact = &mut self.file[at];
        match row_identity(row, contact.target.known_id(), snapshot) {
            Err(reason) => errors.push(reason),
            Ok(None) => {}
            Ok(Some(identity)) => match self.listed.get(&identity.key) {
                Some(&(other, first)) if other != at => errors.push(format!(
                    "row {}: {} is also on row {first} under another contact_id; \
                     an identity belongs to one contact",
                    row.number, identity.key.normalized
                )),
                // The same identity twice under one contact says it once.
                Some(_) => {}
                None => {
                    self.listed.insert(identity.key.clone(), (at, row.number));
                    contact.identities.push(identity);
                }
            },
        }
    }
}

/// Take the name `row` gives `contact`, or refuse the row when an earlier
/// row of the contact gave another.
fn agree_on_name(contact: &mut FileContact, row: &FileRow, errors: &mut Vec<String>) {
    if row.display_name.is_empty() {
        return;
    }
    match &contact.name {
        None => contact.name = Some((row.display_name.clone(), row.number)),
        Some((name, first)) if *name != row.display_name => errors.push(format!(
            "row {}: display_name \"{}\" disagrees with \"{name}\" on row {first}; \
             the rows of contact_id {} must agree or be blank",
            row.number, row.display_name, contact.id_text
        )),
        Some(_) => {}
    }
}

/// Take the Contact Groups `row` lists for `contact`, or refuse the row
/// when a group name is one a person could not create, or when an earlier
/// row of the contact listed other groups.
fn agree_on_groups(contact: &mut FileContact, row: &FileRow, errors: &mut Vec<String>) {
    if row.groups.is_empty() {
        return;
    }
    let names = match row_groups(row) {
        Ok(names) => names,
        Err(reason) => return errors.push(reason),
    };
    match &contact.groups {
        None => contact.groups = Some((names, row.number)),
        Some((first_names, first)) if !same_groups(first_names, &names) => {
            errors.push(format!(
                "row {}: groups \"{}\" disagrees with \"{}\" on row {first}; \
                 the rows of contact_id {} must agree or be blank",
                row.number,
                names.join("; "),
                first_names.join("; "),
                contact.id_text
            ));
        }
        Some(_) => {}
    }
}

/// Refuse a new contact that has neither a name nor an identity, which
/// nothing could ever reach.
fn check_new_contact_has_a_name_or_identity(contact: &FileContact, errors: &mut Vec<String>) {
    if contact.target == Target::New && contact.name.is_none() && contact.identities.is_empty() {
        errors.push(format!(
            "row {}: a new contact needs a display_name or an identity",
            contact.first_row
        ));
    }
}

/// Refuse each identity of `contact` that another contact holds and the
/// load may not take from it: a named contact outside the file (`in_file`),
/// or one in the Trash.
fn check_identities_may_move(
    contact: &FileContact,
    snapshot: &Snapshot,
    in_file: &HashSet<i64>,
    errors: &mut Vec<String>,
) {
    for identity in &contact.identities {
        let Some(&(_, Some(holder))) = snapshot.handles.get(&identity.key) else {
            continue;
        };
        if contact.target == Target::Known(holder) {
            continue;
        }
        if snapshot.may_take_from(holder, in_file) {
            continue;
        }
        let holder_name = snapshot.name_of(holder);
        // A trashed holder cannot be added to the file, because its id
        // reads as unknown text, so the way through is the Trash.
        let (where_it_is, way_through) = if snapshot.trashed.contains_key(&holder) {
            (
                "is in the Trash",
                format!(
                    "restore \"{holder_name}\" and add it to the file, \
                     or delete \"{holder_name}\" for good,"
                ),
            )
        } else {
            (
                "is not in the file",
                format!("add \"{holder_name}\" to the file"),
            )
        };
        errors.push(format!(
            "row {}: {} belongs to \"{holder_name}\" (contact {holder}), which {where_it_is}, \
             so it cannot move to {}; {way_through} to move it",
            identity.row,
            identity.key.normalized,
            contact.describe()
        ));
    }
}

/// The contacts the account holds that the file speaks for.
fn known_contacts(file: &[FileContact]) -> HashSet<i64> {
    file.iter().filter_map(|c| c.target.known_id()).collect()
}

/// Check every row and gather the rows into contacts. Every broken rule is
/// collected, so one refusal names them all.
fn plan(rows: &[FileRow], snapshot: &Snapshot) -> Result<Vec<FileContact>, Vec<String>> {
    let mut errors: Vec<String> = Vec::new();
    let mut gathering = Gathering::default();
    for row in rows {
        let at = gathering.contact_of(row, snapshot);
        agree_on_name(&mut gathering.file[at], row, &mut errors);
        agree_on_groups(&mut gathering.file[at], row, &mut errors);
        gathering.list_identity(at, row, snapshot, &mut errors);
    }

    let file = gathering.file;
    let in_file = known_contacts(&file);
    for contact in &file {
        check_new_contact_has_a_name_or_identity(contact, &mut errors);
        check_identities_may_move(contact, snapshot, &in_file, &mut errors);
    }

    if errors.is_empty() {
        Ok(file)
    } else {
        Err(errors)
    }
}

/// Load an address book into the account.
///
/// The whole load is one transaction. A file that breaks a rule is refused
/// whole, with one sentence for each bad row, and nothing is written. Each
/// contact the load changes is stamped `now`.
///
/// An Edit load deletes for good the identities the file no longer lists
/// that nothing else uses, so after it commits the write-ahead log is
/// cleared of them ([`crate::db::erase`]). An Append load deletes no text
/// for good: the only rows it deletes are nameless contacts its moves leave
/// without an identity, which hold none. It leaves the log as it is, so a
/// name it replaces stays in the log until a later checkpoint; a rename is
/// not a delete for good.
///
/// # Errors
///
/// [`LoadError::Refused`] when the file breaks a rule; [`LoadError::Failed`]
/// when a statement fails.
pub async fn load(
    conn: &mut SqliteConnection,
    account_id: i64,
    csv_text: &str,
    mode: LoadMode,
    now: DateTime<Utc>,
) -> Result<LoadCounts, LoadError> {
    let rows = read_rows(csv_text).map_err(LoadError::Refused)?;
    // A write transaction from the first read: the plan is checked against
    // the contacts the writes then change, and an import that commits
    // meanwhile waits instead of failing the load.
    let mut tx = begin_write(conn)
        .await
        .context("begin a write transaction")?;
    let snapshot = Snapshot::read(&mut tx, account_id).await?;
    let file = plan(&rows, &snapshot).map_err(LoadError::Refused)?;
    let counts = apply(&mut tx, account_id, &snapshot, &file, mode, now).await?;
    tx.commit().await.context("commit the address book load")?;
    if mode == LoadMode::Edit {
        crate::db::erase::clear_write_ahead_log(conn).await;
    }
    Ok(counts)
}

/// The address book `csv_text` rewritten so that each new contact it lists
/// carries the id of the nameless contact that holds one of its identities,
/// as a person who exported the address book after an import and typed the
/// names onto the rows of its Unknowns would write it. Only the text is
/// rewritten; the account is read and nothing is written. Loaded back, the
/// file names those nameless contacts in place, where a new contact would
/// take their identities and leave them to be deleted.
///
/// The test is the name alone: a contact with a name is already named, and
/// a nameless one is what the file's name is for.
///
/// It is written for the demo address book, whose every contact has a
/// `contact_id` key of its own and whose rows are never blank. A new contact
/// with a blank `contact_id` stays new, because its rows have no text to
/// rewrite that only they share. A blank row is left out, as the load leaves
/// it out.
///
/// The file is read as [`load`] reads it, so the identities are matched by
/// the key the load would give them. A new contact stays new when no
/// nameless contact holds its identities; when the nameless contact also
/// holds an identity the contact's rows do not list (an Append load would
/// then leave the named contact holding it); or when a row would read as
/// another identity under the nameless contact's id than as a new
/// contact's. A file the load would refuse comes back as it was, so the load
/// reports the refusal.
///
/// # Errors
///
/// Returns an error when reading the account fails.
pub(crate) async fn rewrite_ids_to_nameless(
    conn: &mut SqliteConnection,
    account_id: i64,
    csv_text: &str,
) -> Result<String> {
    let Ok(rows) = read_rows(csv_text) else {
        return Ok(csv_text.to_string());
    };
    let snapshot = Snapshot::read(conn, account_id).await?;
    let Ok(file) = plan(&rows, &snapshot) else {
        return Ok(csv_text.to_string());
    };

    let mut held: HashMap<i64, HashSet<&IdentityKey>> = HashMap::new();
    for (key, &(_, holder)) in &snapshot.handles {
        if let Some(holder) = holder {
            held.entry(holder).or_default().insert(key);
        }
    }
    let is_nameless = |id: i64| snapshot.contacts.get(&id).is_some_and(String::is_empty);

    // The nameless contact each new contact takes, by its `contact_id` text.
    // No two new contacts can take the same one: it must hold only
    // identities the contact lists, and the load refuses a file that lists
    // one identity under two contacts.
    let mut nameless_of: HashMap<&str, i64> = HashMap::new();
    for contact in file
        .iter()
        .filter(|c| c.target == Target::New && !c.id_text.is_empty())
    {
        let listed: HashSet<&IdentityKey> = contact.identities.iter().map(|i| &i.key).collect();
        let contact_rows: Vec<&FileRow> = rows
            .iter()
            .filter(|row| row.contact_id == contact.id_text)
            .collect();
        // Under the nameless contact's id, a phone written without `+` can
        // read as another key (see [`row_identity`]); the rows must read the
        // same.
        let reads_the_same = |nameless: i64| {
            let mut keys = HashSet::new();
            for row in &contact_rows {
                match row_identity(row, Some(nameless), &snapshot) {
                    Ok(Some(identity)) => {
                        keys.insert(identity.key);
                    }
                    Ok(None) => {}
                    Err(_) => return false,
                }
            }
            keys.len() == listed.len() && keys.iter().all(|key| listed.contains(key))
        };
        let nameless = contact.identities.iter().find_map(|identity| {
            let &(_, Some(holder)) = snapshot.handles.get(&identity.key)? else {
                return None;
            };
            let holds_only_listed = held
                .get(&holder)
                .is_some_and(|keys| keys.iter().all(|key| listed.contains(key)));
            (is_nameless(holder) && holds_only_listed && reads_the_same(holder)).then_some(holder)
        });
        if let Some(nameless) = nameless {
            nameless_of.insert(contact.id_text.as_str(), nameless);
        }
    }

    let book = rows.iter().map(|row| {
        [
            nameless_of
                .get(row.contact_id.as_str())
                .map_or_else(|| row.contact_id.clone(), i64::to_string),
            row.display_name.clone(),
            row.service.clone(),
            row.identity.clone(),
            row.groups.clone(),
        ]
    });
    write_book(book)
}

/// The `contact_id` cell of each data row of an address book file, read with
/// the CSV reader the load uses.
#[cfg(test)]
pub(crate) fn contact_ids_of(text: &str) -> Vec<String> {
    csv::Reader::from_reader(text.as_bytes())
        .records()
        .map(|record| {
            record
                .expect("an address book row")
                .get(0)
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

/// The address book file of `rows`, each in the order of [`COLUMNS`], under
/// the header, with every cell written as [`message_crate_csv::written_cell`]
/// writes it.
fn write_book(rows: impl IntoIterator<Item = [String; 5]>) -> Result<String> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(COLUMNS)?;
    for row in rows {
        write_guarded_record(&mut writer, row)?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|e| anyhow::anyhow!("finish the address book: {e}"))?;
    String::from_utf8(bytes).context("the address book is not UTF-8")
}

/// What a load tracks as it writes a checked file, which each step of
/// [`apply`] reads and changes.
struct LoadState<'a> {
    account_id: i64,
    snapshot: &'a Snapshot,
    mode: LoadMode,
    counts: LoadCounts,
    /// Contact Groups by lower-cased name, with the ones this load created.
    groups: HashMap<String, i64>,
    /// Who holds each identity as the load goes: it changes as rows move them.
    holder_of: HashMap<i64, i64>,
    /// Contacts this load changed.
    changed: HashSet<i64>,
    /// Contacts this load took an identity from.
    lost_identity: HashSet<i64>,
    /// Each file contact's id and the identity rows it lists.
    placed: Vec<(i64, HashSet<i64>)>,
}

impl<'a> LoadState<'a> {
    fn new(account_id: i64, snapshot: &'a Snapshot, mode: LoadMode, contacts: usize) -> Self {
        Self {
            account_id,
            snapshot,
            mode,
            counts: LoadCounts::default(),
            groups: snapshot.groups.clone(),
            holder_of: snapshot
                .handles
                .values()
                .filter_map(|&(handle_id, holder)| holder.map(|h| (handle_id, h)))
                .collect(),
            changed: HashSet::new(),
            lost_identity: HashSet::new(),
            placed: Vec::with_capacity(contacts),
        }
    }
}

/// Write a checked file, stamping each contact it changes `now`. Nothing
/// here refuses: [`plan`] already has.
///
/// The steps run in this order, each reading what the ones before it left
/// in [`LoadState`]. [`release_unlisted_identities`] needs every file
/// contact placed. [`delete_empty_contacts`] reads the contacts that placing
/// and releasing took an identity from, and takes each one it deletes out of
/// the changed contacts, so it runs before [`stamp_changed_contacts`], which
/// stamps the changed contacts that are left.
async fn apply(
    tx: &mut WriteTx<'_>,
    account_id: i64,
    snapshot: &Snapshot,
    file: &[FileContact],
    mode: LoadMode,
    now: DateTime<Utc>,
) -> Result<LoadCounts> {
    let conn: &mut SqliteConnection = tx;
    let mut state = LoadState::new(account_id, snapshot, mode, file.len());
    place_file_contacts(conn, &mut state, file, now).await?;
    release_unlisted_identities(conn, &mut state).await?;
    delete_empty_contacts(conn, &mut state).await?;
    stamp_changed_contacts(conn, &mut state, file, now).await?;
    state.counts.notes = notes_in_row_order(file);
    Ok(state.counts)
}

/// Every file contact takes what its rows list: its name, its identities
/// and its Contact Groups. Removal of an identity waits until all of them
/// have, so an identity that goes from one file contact to another is one
/// move, whichever of the two comes first in the file.
async fn place_file_contacts(
    conn: &mut SqliteConnection,
    state: &mut LoadState<'_>,
    file: &[FileContact],
    now: DateTime<Utc>,
) -> Result<()> {
    for contact in file {
        let contact_id = name_or_create(conn, state, contact, now).await?;
        let listed = take_identities(conn, state, contact, contact_id).await?;
        set_groups(conn, state, contact, contact_id).await?;
        state.placed.push((contact_id, listed));
    }
    Ok(())
}

/// The id of the contact `contact` speaks for: a known contact given the
/// file's name, or a contact created for it.
async fn name_or_create(
    conn: &mut SqliteConnection,
    state: &mut LoadState<'_>,
    contact: &FileContact,
    now: DateTime<Utc>,
) -> Result<i64> {
    match contact.target {
        Target::Known(id) => {
            // The file is the person typing, so its name replaces
            // whatever the contact carried; `propose_name` holds the rule.
            if let Some((name, _)) = &contact.name
                && contacts::propose_name(
                    conn,
                    state.account_id,
                    id,
                    name,
                    Origin::AddressBook,
                    now,
                )
                .await?
            {
                state.changed.insert(id);
            }
            Ok(id)
        }
        Target::New => {
            let name = contact.name.as_ref().map_or("", |(name, _)| name.as_str());
            state.counts.contacts_created += 1;
            contacts::create_contact(conn, state.account_id, name, Origin::AddressBook).await
        }
    }
}

/// Give `contact_id` each identity `contact` lists: a new one is created,
/// one with no holder is linked, and one another contact holds moves.
/// Returns the identity rows the contact lists.
async fn take_identities(
    conn: &mut SqliteConnection,
    state: &mut LoadState<'_>,
    contact: &FileContact,
    contact_id: i64,
) -> Result<HashSet<i64>> {
    let account_id = state.account_id;
    let mut listed: HashSet<i64> = HashSet::new();
    for identity in &contact.identities {
        let handle_id = match state.snapshot.handles.get(&identity.key) {
            Some(&(handle_id, _)) => handle_id,
            None => sqlx::query_scalar(
                "INSERT INTO handles
                       (account_id, raw, normalized, handle_type, service, origin)
                     VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
            )
            .bind(account_id)
            .bind(&identity.written)
            .bind(&identity.key.normalized)
            .bind(identity.key.handle_type)
            .bind(identity.key.service.as_str())
            .bind(Origin::AddressBook.as_str())
            .fetch_one(&mut *conn)
            .await
            .context("insert an identity from the address book")?,
        };
        listed.insert(handle_id);
        match state.holder_of.insert(handle_id, contact_id) {
            Some(holder) if holder == contact_id => {}
            Some(holder) => {
                let goes = contacts::IdentityGoes::To(contact_id, Origin::AddressBook);
                contacts::move_identity(conn, account_id, handle_id, goes).await?;
                state.counts.identities_moved += 1;
                state.lost_identity.insert(holder);
                state.changed.insert(holder);
                state.changed.insert(contact_id);
            }
            None => {
                contacts::link_handle_to_contact(
                    conn,
                    account_id,
                    handle_id,
                    contact_id,
                    Origin::AddressBook,
                )
                .await?;
                state.counts.identities_added += 1;
                state.changed.insert(contact_id);
            }
        }
    }
    Ok(listed)
}

/// Make `contact_id` a member of each Contact Group `contact` lists,
/// creating a group no name matches, and in Edit take it out of every
/// other group.
async fn set_groups(
    conn: &mut SqliteConnection,
    state: &mut LoadState<'_>,
    contact: &FileContact,
    contact_id: i64,
) -> Result<()> {
    let mut wanted: HashSet<i64> = HashSet::new();
    for name in contact.groups.iter().flat_map(|(names, _)| names) {
        let group_id = match state.groups.get(&name.to_lowercase()) {
            Some(&id) => id,
            None => {
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO contact_groups (account_id, name) VALUES ($1, $2) RETURNING id",
                )
                .bind(state.account_id)
                .bind(name)
                .fetch_one(&mut *conn)
                .await
                .context("insert a Contact Group from the address book")?;
                state.groups.insert(name.to_lowercase(), id);
                state.counts.groups_created += 1;
                id
            }
        };
        wanted.insert(group_id);
    }
    let held = state.snapshot.memberships.get(&contact_id);
    for &group_id in &wanted {
        if held.is_some_and(|h| h.contains(&group_id)) {
            continue;
        }
        sqlx::query(
            "INSERT INTO contact_group_members (contact_id, group_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(contact_id)
        .bind(group_id)
        .execute(&mut *conn)
        .await
        .context("add a contact to a Contact Group")?;
        state.changed.insert(contact_id);
    }
    if state.mode == LoadMode::Edit {
        for &group_id in held.into_iter().flatten() {
            if wanted.contains(&group_id) {
                continue;
            }
            sqlx::query(
                "DELETE FROM contact_group_members WHERE contact_id = $1 AND group_id = $2",
            )
            .bind(contact_id)
            .bind(group_id)
            .execute(&mut *conn)
            .await
            .context("take a contact out of a Contact Group")?;
            state.changed.insert(contact_id);
        }
    }
    Ok(())
}

/// Edit: an identity a file contact still holds and no row of it lists
/// comes off the contact, the one way an identity leaves a contact: one in
/// a conversation goes to a new contact with no name, so the person is
/// Unknown for it again, and one nothing uses is deleted.
async fn release_unlisted_identities(
    conn: &mut SqliteConnection,
    state: &mut LoadState<'_>,
) -> Result<()> {
    if state.mode != LoadMode::Edit {
        return Ok(());
    }
    for (contact_id, listed) in &state.placed {
        let mut unlisted: Vec<i64> = state
            .holder_of
            .iter()
            .filter(|&(handle_id, holder)| holder == contact_id && !listed.contains(handle_id))
            .map(|(&handle_id, _)| handle_id)
            .collect();
        unlisted.sort_unstable();
        contacts::take_identities_off(conn, state.account_id, &unlisted).await?;
        for handle_id in unlisted {
            state.holder_of.remove(&handle_id);
            state.counts.identities_removed += 1;
            state.lost_identity.insert(*contact_id);
            state.changed.insert(*contact_id);
        }
    }
    Ok(())
}

/// A contact this load left with neither a name nor an identity is one
/// nothing could ever reach, so it goes.
async fn delete_empty_contacts(
    conn: &mut SqliteConnection,
    state: &mut LoadState<'_>,
) -> Result<()> {
    for contact_id in std::mem::take(&mut state.lost_identity) {
        if contacts::delete_if_empty(conn, state.account_id, contact_id).await? {
            state.changed.remove(&contact_id);
            state.counts.contacts_deleted += 1;
        }
    }
    Ok(())
}

/// Stamp each contact the load changed `now`, and count the ones the file
/// speaks for as updated.
async fn stamp_changed_contacts(
    conn: &mut SqliteConnection,
    state: &mut LoadState<'_>,
    file: &[FileContact],
    now: DateTime<Utc>,
) -> Result<()> {
    let in_file = known_contacts(file);
    for contact_id in std::mem::take(&mut state.changed) {
        contacts::touch_contact(conn, state.account_id, contact_id, now).await?;
        if in_file.contains(&contact_id) {
            state.counts.contacts_updated += 1;
        }
    }
    Ok(())
}

/// The notes of the file's identities, in the order of the file's rows,
/// which a contact's rows need not be.
fn notes_in_row_order(file: &[FileContact]) -> Vec<String> {
    let mut notes: Vec<(usize, &String)> = file
        .iter()
        .flat_map(|contact| &contact.identities)
        .filter_map(|identity| identity.note.as_ref().map(|note| (identity.row, note)))
        .collect();
    notes.sort_by_key(|&(row, _)| row);
    notes.into_iter().map(|(_, note)| note.clone()).collect()
}

/// An address book [`export_csv`] wrote, with how many contacts and
/// identities it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenAddressBook {
    /// The CSV text.
    pub csv: String,
    /// The distinct contacts the file holds.
    pub contacts: u64,
    /// The rows that carry an identity.
    pub identities: u64,
}

/// One row of [`export_csv`]'s query: the contact's id and name, and one of
/// its identities as service, handle type and key, absent for a contact
/// with no identity.
#[derive(sqlx::FromRow)]
struct ExportRow {
    id: i64,
    name: String,
    service: Option<String>,
    handle_type: Option<String>,
    normalized: Option<String>,
}

/// One Contact Group a contact is in, for [`export_csv`].
#[derive(sqlx::FromRow)]
struct ExportMembership {
    contact_id: i64,
    name: String,
}

/// Write the address book for the account's contacts, or for the ones in
/// `only` when it is given. One row per identity; a contact with no identity
/// is one row with `service` and `identity` blank. Contacts in the trash are
/// left out, as they are from Contacts.
///
/// Rows are ordered by name, a contact with no name first, so the Unknowns
/// a person exports to name sit together at the top.
///
/// # Errors
///
/// Returns an error when a query fails.
pub async fn export_csv(
    conn: &mut SqliteConnection,
    account_id: i64,
    only: Option<&HashSet<i64>>,
) -> Result<WrittenAddressBook> {
    let rows = sqlx::query_as::<_, ExportRow>(sqlx::AssertSqlSafe(format!(
        "SELECT ct.id, ct.preferred_name AS name, h.service, h.handle_type, h.normalized
         FROM contacts ct
         LEFT JOIN contact_handles ch
           ON ch.account_id = ct.account_id AND ch.contact_id = ct.id
         LEFT JOIN handles h ON h.id = ch.handle_id
         WHERE ct.account_id = $1
           AND {not_trashed}
         ORDER BY lower(ct.preferred_name), ct.id,
                  h.handle_type, h.service, h.normalized",
        not_trashed = not_trashed_contact("ct"),
    )))
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .context("read the contacts and identities to export")?;
    let memberships = sqlx::query_as::<_, ExportMembership>(
        "SELECT m.contact_id, g.name
         FROM contact_group_members m
         JOIN contact_groups g ON g.id = m.group_id
         WHERE g.account_id = $1
         ORDER BY lower(g.name)",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .context("read the Contact Group memberships to export")?;
    let mut groups: HashMap<i64, Vec<String>> = HashMap::new();
    for ExportMembership { contact_id, name } in memberships {
        groups.entry(contact_id).or_default().push(name);
    }

    let rows: Vec<ExportRow> = rows
        .into_iter()
        .filter(|row| only.is_none_or(|only| only.contains(&row.id)))
        .collect();
    let contacts: HashSet<i64> = rows.iter().map(|row| row.id).collect();
    let identities = rows.iter().filter(|row| row.normalized.is_some()).count();
    let book = rows.into_iter().map(
        |ExportRow {
             id,
             name,
             service,
             handle_type,
             normalized,
         }| {
            let group_names = groups
                .get(&id)
                .map(|names| names.join(&GROUP_SEPARATOR.to_string()))
                .unwrap_or_default();
            let word = service
                .as_deref()
                .and_then(parse_service)
                .zip(handle_type.as_deref().and_then(parse_handle_type))
                .map(|(service, handle_type)| FileService::of(service, handle_type).word());
            [
                id.to_string(),
                name,
                word.unwrap_or_default().to_string(),
                normalized.unwrap_or_default(),
                group_names,
            ]
        },
    );
    Ok(WrittenAddressBook {
        csv: write_book(book)?,
        contacts: u64::try_from(contacts.len()).unwrap_or(u64::MAX),
        identities: u64::try_from(identities).unwrap_or(u64::MAX),
    })
}

/// One row per `TEL` and `EMAIL` on each card of the vCard file `text`, as
/// the address book a person edits before a load: `contact_id` is `card1`,
/// `card2`, … by the card's position in the file, so a card's rows load as one
/// new contact; `display_name` is the card's name ([`::contacts::VcfCard::display_name`]);
/// `service` is `Text Message` for a number and `Email` for an email
/// address, never `WhatsApp`, because a card cannot say a number is on
/// WhatsApp; and `groups` is blank, because `CATEGORIES` is not carried over.
///
/// A number written without `+` is written in its `+` form in `country`
/// when it is a valid number there. A number written with `+`, one that is
/// not valid in `country`, and every number when `country` is `None` is
/// written exactly as the card has it, for the load to key. Nothing is
/// checked beyond that: the load refuses what it cannot store and names the
/// rows. A card with no number and no email address gives no rows.
///
/// `Ok(None)` when `text` holds no card at all.
///
/// # Errors
///
/// [`VcardError::UnknownCharset`] when a card writes its name, a number or
/// an email address in a charset Message Crate cannot read, and
/// [`VcardError::Failed`] when the CSV cannot be written.
pub fn from_vcard(
    text: &str,
    country: Option<&'static phone::Country>,
) -> Result<Option<String>, VcardError> {
    let cards = ::contacts::parse_vcf(text)?;
    if cards.is_empty() {
        return Ok(None);
    }
    let mut rows = Vec::new();
    for (at, card) in cards.iter().enumerate() {
        let contact_id = format!("card{}", at + 1);
        let name = card.display_name();
        let row = |service: FileService, identity: String| {
            [
                contact_id.clone(),
                name.clone(),
                service.word().to_string(),
                identity,
                String::new(),
            ]
        };
        for number in &card.phones {
            rows.push(row(FileService::TextMessage, number_in(number, country)));
        }
        for email in &card.emails {
            rows.push(row(FileService::Email, email.clone()));
        }
    }
    Ok(Some(write_book(rows)?))
}

/// Why a vCard file did not convert to an address book.
#[derive(Debug, thiserror::Error)]
pub enum VcardError {
    /// A card writes a value in a charset Message Crate cannot read; the
    /// person can fix the file.
    #[error(transparent)]
    UnknownCharset(#[from] ::contacts::UnknownCharset),
    /// Something failed that changing the file would not help.
    #[error(transparent)]
    Failed(#[from] anyhow::Error),
}

/// The number `written` as the address book writes it: its `+` form in
/// `country` when it is written without `+` and is a valid number there,
/// and as written otherwise.
fn number_in(written: &str, country: Option<&'static phone::Country>) -> String {
    let Some(country) = country else {
        return written.to_string();
    };
    let is_a_number =
        phone::Handle::parse(written).is_some_and(|handle| handle.kind() == IdentityType::Phone);
    if !is_a_number || written.contains('+') {
        return written.to_string();
    }
    phone::normalize_checked(written, phone::PhoneRegion::Country(country))
        .unwrap_or_else(|_| written.to_string())
}

#[cfg(test)]
mod tests;
