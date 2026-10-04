//! Writes the demo's address book and the small config files the server reads
//! when it builds the Demo Account.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::personas::{OWNER_EMAIL, OWNER_PHONE, Roster};

/// The columns of the address book, as the server's Export writes them.
const ADDRESS_BOOK_HEADER: &str = "contact_id,display_name,groups,service,identity_type,identity";

/// Write `contacts.csv`, the demo's address book, in the format the server
/// exports and loads: one row per identity.
///
/// The demo is built the way a person builds theirs. The three backups are
/// imported first and carry no names for these people, so each arrives as an
/// Unknown. Every contact here has a key of the file's own (`demo-1`,
/// `demo-2`, ...). The server's Demo Account build replaces each key with the
/// id of the Unknown the imports made for that contact, as a person who
/// exported the address book would find it, and loads the file, which names
/// those Unknowns in place. A contact on WhatsApp lists its number once for
/// each service, so the named contact holds every identity its Unknown held.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write_address_book(config_dir: &Path, roster: &Roster) -> Result<()> {
    let contacts_path = config_dir.join("contacts.csv");
    let mut out = String::from(ADDRESS_BOOK_HEADER);
    out.push('\n');
    for (index, c) in roster.contacts.iter().enumerate() {
        let lead = format!(
            "{},{},{}",
            contact_id(index),
            csv_field(&c.display_hint()),
            csv_field(&c.groups.join(";"))
        );
        for phone in &c.phones {
            writeln!(out, "{lead},phone,phone,{}", csv_field(phone))?;
        }
        if c.has_whatsapp {
            writeln!(
                out,
                "{lead},whatsapp,phone,{}",
                csv_field(c.primary_phone())
            )?;
        }
    }
    fs::write(&contacts_path, out).with_context(|| format!("write {}", contacts_path.display()))?;
    Ok(())
}

/// The address book's own key for the contact at `index` of the roster:
/// `demo-1`, `demo-2`, ...
fn contact_id(index: usize) -> String {
    format!("demo-{}", index + 1)
}

/// One CSV field: quoted when it holds a comma, a quote, or a line break.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Write `seed.toml` with the demo account name, phone, and username.
///
/// The server reads this file only when it builds the Demo Account:
/// `reset-demo`, `serve` on a new database, and the Owner Home action.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write_seed_toml(config_dir: &Path) -> Result<()> {
    let path = config_dir.join("seed.toml");
    let body = format!(
        r#"# Demo account identity, read only when the server builds the Demo Account
# (`reset-demo`, `serve` on a new database, the Owner Home action).

[owner]
display_name = "Demo User"
# (raw handle, handle type) pairs linked into `account_handles` by reset-demo.
handle_specs = [["{OWNER_PHONE}", "phone"]]
# Each email is an identity too: reset-demo links it into `account_handles`.
emails = ["{OWNER_EMAIL}"]

[account]
username = "demo"
"#
    );
    fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DemoSize, SeedConfig};
    use crate::names::NameBank;
    use crate::seeded_roster;

    /// A name holding a comma or a quote stays one field, so the server's
    /// load reads the name and the groups from their own columns.
    #[test]
    fn csv_field_quotes_a_comma_and_doubles_a_quote() {
        assert_eq!(csv_field("Ann Lee"), "Ann Lee");
        assert_eq!(csv_field("Ann \"Nan\", Lee"), "\"Ann \"\"Nan\"\", Lee\"");
        assert_eq!(csv_field("two\nlines"), "\"two\nlines\"");
    }

    /// The server's load names an Unknown only with the name its row gives,
    /// so a row with a blank name names nobody and leaves its number on an
    /// Unknown, where the unassigned handles already put numbers nobody
    /// named (#1557). `write_address_book` writes each contact's
    /// `display_hint` as the name of every row it has.
    #[test]
    fn every_contact_in_the_medium_and_large_address_books_has_a_name() {
        let names = NameBank::load_default().expect("load the name lists");
        for size in [DemoSize::Medium, DemoSize::Large] {
            let cfg = SeedConfig::for_size(size).expect("load the settings");
            let (roster, _) = seeded_roster(&cfg, &names).expect("build the roster");
            assert_eq!(roster.contacts.len(), cfg.contacts.count);
            let blank: Vec<String> = roster
                .contacts
                .iter()
                .enumerate()
                .filter(|(_, c)| c.display_hint().trim().is_empty())
                .map(|(index, _)| contact_id(index))
                .collect();
            assert!(
                blank.is_empty(),
                "the {} address book has contacts with no name: {blank:?}",
                size.as_str()
            );
        }
    }
}
