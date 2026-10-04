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
            "demo-{},{},{}",
            index + 1,
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
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    use super::*;
    use crate::config::{DemoSize, SeedConfig};
    use crate::names::NameBank;
    use crate::personas::build_roster;

    /// The fields of one address book row, quoted fields unquoted.
    fn csv_row(line: &str) -> Vec<String> {
        let mut fields = vec![String::new()];
        let mut quoted = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, quoted) {
                ('"', true) if chars.peek() == Some(&'"') => {
                    chars.next();
                    fields.last_mut().expect("a field").push('"');
                }
                ('"', _) => quoted = !quoted,
                (',', false) => fields.push(String::new()),
                (c, _) => fields.last_mut().expect("a field").push(c),
            }
        }
        fields
    }

    #[test]
    fn csv_row_reads_a_quoted_field_with_a_comma_and_a_quote() {
        let line = format!("demo-1,{},Family", csv_field("Ann \"Nan\", Lee"));
        assert_eq!(csv_row(&line), ["demo-1", "Ann \"Nan\", Lee", "Family"]);
    }

    /// The server's load names an Unknown only with the name its row gives,
    /// so a row with a blank name names nobody and leaves its number on an
    /// Unknown, where the Unassigned handles already put numbers nobody
    /// named (#1557).
    #[test]
    fn every_contact_in_the_medium_and_large_address_books_has_a_name() {
        let names = NameBank::load_default().expect("load the name lists");
        for size in [DemoSize::Medium, DemoSize::Large] {
            let cfg = SeedConfig::for_size(size).expect("load the settings");
            // The roster is the first thing the generator draws from its
            // seed, so this is the roster the bundle's address book holds.
            let mut rng = ChaCha8Rng::seed_from_u64(cfg.seed);
            let roster = build_roster(&cfg, &names, &mut rng).expect("build the roster");
            let temp = tempfile::tempdir().expect("create test directory");
            write_address_book(temp.path(), &roster).expect("write the address book");

            let text = fs::read_to_string(temp.path().join("contacts.csv"))
                .expect("read the address book");
            let mut lines = text.lines();
            assert_eq!(lines.next(), Some(ADDRESS_BOOK_HEADER));
            let rows: Vec<Vec<String>> = lines.map(csv_row).collect();
            assert!(
                !rows.is_empty(),
                "the {} address book has rows",
                size.as_str()
            );
            let blank: Vec<&str> = rows
                .iter()
                .filter(|row| row[1].trim().is_empty())
                .map(|row| row[0].as_str())
                .collect();
            assert!(
                blank.is_empty(),
                "the {} address book has contacts with no name: {blank:?}",
                size.as_str()
            );
        }
    }
}
