//! What the server reads from a demo bundle when it builds the Demo Account:
//! the three source names, which name the directories under `staging/` and
//! the source of every conversation in them, and `config/seed.toml`.
//!
//! demo-seed writes the bundle and the server reads it, and both use the
//! names and types here, so a renamed source or a new `seed.toml` key is
//! changed in one place.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use message_ir::IdentityType;
use serde::{Deserialize, Serialize};

use crate::personas::{OWNER_EMAIL, OWNER_PHONE};

/// The Apple Messages source.
pub const IMESSAGE_SOURCE: &str = "imessage";
/// The SMS Backup & Restore source.
pub const SBR_SOURCE: &str = "sms-backup-restore";
/// The WhatsApp source.
pub const WHATSAPP_SOURCE: &str = "whatsapp";

/// The lines `write_seed_toml` puts above the values. `toml::to_string`
/// writes no comments, so the header says what each key does.
const SEED_TOML_HEADER: &str = "\
# Demo account identity, read only when the server builds the Demo Account
# (`reset-demo`, `serve` on a new database, the Owner Home action).
#
# owner.display_name: the Demo Account's display name.
# owner.handle_specs: (raw handle, handle type) pairs linked into
#   `account_handles`.
# owner.emails: each email is an identity too, linked into `account_handles`.

";

/// `config/seed.toml` in a demo bundle. A key it does not use is an error,
/// never ignored (`deny_unknown_fields` here and on its section): a misspelled
/// key would seed the Demo Account without what the line asked for.
///
/// It names no username. The Demo Account's username is fixed in the server,
/// which reserves it while the account is absent
/// (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`), so an
/// `[account]` section is refused like any other unknown key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoSeed {
    /// The person the Demo Account belongs to.
    pub owner: DemoOwner,
}

/// `[owner]` in `seed.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoOwner {
    /// The Demo Account's display name.
    pub display_name: String,
    /// `(raw handle, handle type)` pairs linked into `account_handles`.
    #[serde(default)]
    pub handle_specs: Vec<(String, IdentityType)>,
    /// Email identities, linked into `account_handles`.
    #[serde(default)]
    pub emails: Vec<String>,
}

impl DemoSeed {
    /// The values every generated bundle carries: the owner's phone number
    /// and email address, which the conversations send from.
    #[must_use]
    pub fn demo() -> Self {
        Self {
            owner: DemoOwner {
                display_name: "Demo User".into(),
                handle_specs: vec![(OWNER_PHONE.into(), IdentityType::Phone)],
                emails: vec![OWNER_EMAIL.into()],
            },
        }
    }
}

/// Write `seed.toml` in `config_dir` with [`DemoSeed::demo`].
///
/// The server reads this file only when it builds the Demo Account:
/// `reset-demo`, `serve` on a new database, and the Owner Home action.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write_seed_toml(config_dir: &Path) -> Result<()> {
    let path = config_dir.join("seed.toml");
    let values = toml::to_string(&DemoSeed::demo()).context("serialize seed.toml")?;
    fs::write(&path, format!("{SEED_TOML_HEADER}{values}"))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Read the `seed.toml` at `path`.
///
/// # Errors
///
/// Returns an error, naming the file, if it cannot be read or holds a key
/// or section [`DemoSeed`] does not have.
pub fn read_seed_toml(path: &Path) -> Result<DemoSeed> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read demo seed {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("failed to parse demo seed {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `seed.toml` [`write_seed_toml`] writes, with `find` replaced by
    /// `replace`, and the error reading it back.
    fn refusal_of_written_seed_with(find: &str, replace: &str) -> String {
        let dir = tempfile::tempdir().expect("temp dir");
        write_seed_toml(dir.path()).expect("write seed.toml");
        let path = dir.path().join("seed.toml");
        let text = fs::read_to_string(&path).expect("read seed.toml");
        assert!(text.contains(find), "the written seed.toml holds {find:?}");
        fs::write(&path, text.replacen(find, replace, 1)).expect("rewrite seed.toml");
        format!("{:#}", read_seed_toml(&path).unwrap_err())
    }

    /// The header and the `toml::to_string` output parse back under
    /// `deny_unknown_fields` into the values written.
    #[test]
    fn the_written_seed_toml_reads_back_into_the_same_values() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_seed_toml(dir.path()).expect("write seed.toml");

        let read = read_seed_toml(&dir.path().join("seed.toml")).expect("read seed.toml");

        assert_eq!(read, DemoSeed::demo());
    }

    /// A key the server does not use is refused by its name, with the file: a
    /// misspelled `emails` otherwise seeds a Demo Account with no email identity.
    #[test]
    fn a_seed_toml_with_an_unknown_key_or_section_is_refused_naming_it() {
        let text = refusal_of_written_seed_with("emails = ", "email = ");
        assert!(text.contains("`email`"), "{text}");
        assert!(text.contains("seed.toml"), "{text}");

        let text = refusal_of_written_seed_with("[owner]\n", "[owner]\npassword = \"x\"\n");
        assert!(text.contains("`password`"), "{text}");

        let text = refusal_of_written_seed_with("[owner]\n", "[ownr]\nx = 1\n\n[owner]\n");
        assert!(text.contains("`ownr`"), "{text}");
    }

    /// The Demo Account's username is fixed in the server, not read from the
    /// bundle, so a `seed.toml` that names one is refused (#2808): a name the
    /// server built the account with but did not reserve could be taken by
    /// another account and block the next build.
    #[test]
    fn a_seed_toml_with_an_account_section_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_seed_toml(dir.path()).expect("write seed.toml");
        let path = dir.path().join("seed.toml");
        let text = fs::read_to_string(&path).expect("read seed.toml");
        let text = format!("{text}\n[account]\nusername = \"demo\"\n");
        fs::write(&path, text).expect("rewrite seed.toml");

        let error = format!("{:#}", read_seed_toml(&path).unwrap_err());

        assert!(error.contains("`account`"), "{error}");
        assert!(error.contains("seed.toml"), "{error}");
    }
}
