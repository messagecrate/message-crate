//! WhatsApp's files out of an encrypted iPhone backup.
//!
//! wtsexporter reads an unencrypted iPhone backup by itself. For an
//! encrypted one it asks for the password on a terminal and takes it no
//! other way, so the app decrypts WhatsApp's app-group domain into the
//! run's work directory first (through `imessage-reader`, the only program here
//! that can decrypt an iPhone backup) and wtsexporter reads that directory as
//! it reads a backup someone extracted by hand. The work directory is under the
//! Scratch Directory, and the domain is measured against that disk before
//! anything is decrypted.

use anyhow::{Result, bail};
use ios_backup::{
    DecryptedDomain, decrypt_ios_backup_domain, ios_backup_domain_files, ios_backup_encrypted_flag,
};
use message_crate_core::{ExporterConfig, WhatsappConfig};
use message_staging::{Disk, check_headroom};
use std::path::{Path, PathBuf};

/// The app-group domain WhatsApp keeps its data under in an iPhone backup.
const DOMAIN: &str = "AppDomainGroup-group.net.whatsapp.WhatsApp.shared";
/// The same for the WhatsApp Business app.
const BUSINESS_DOMAIN: &str = "AppDomainGroup-group.net.whatsapp.WhatsAppSMB.shared";

/// Where the decrypted files are.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DecryptedWhatsapp {
    /// The domain's directory: media, contacts and preferences sit under it.
    pub domain_dir: PathBuf,
    /// `ChatStorage.sqlite`, the message database.
    pub database: PathBuf,
}

/// The databases wtsexporter copies beside its extract of a backup that is
/// not encrypted, as well as inside it: so each is written twice.
const DATABASES_COPIED_TWICE: [&str; 3] = [
    "ChatStorage.sqlite",
    "ContactsV2.sqlite",
    "CallHistory.sqlite",
];

/// The bytes wtsexporter writes into its working directory when it extracts
/// WhatsApp's domain from `source.backup`, an iPhone backup that is not
/// encrypted: every file of the domain, and the message, contacts and call
/// databases a second time. `None` when the backup cannot have such an
/// extract: no backup is named, it is encrypted (its files are decrypted,
/// and measured, by [`decrypt_if_encrypted`]), or it is not a backup
/// directory. Whether wtsexporter extracts at all is
/// [`crate::wtsexporter::extracts_ios_backup`].
///
/// # Errors
///
/// Returns an error when the backup's `Manifest.db` cannot be read.
pub(crate) fn extract_bytes(source: &WhatsappConfig) -> Result<Option<u64>> {
    let Some(backup) = source.backup.as_deref() else {
        return Ok(None);
    };
    if ios_backup_encrypted_flag(backup) != Some(false) {
        return Ok(None);
    }
    let files = ios_backup_domain_files(backup, domain(source))?;
    let bytes = files
        .iter()
        .map(|(path, size)| {
            if DATABASES_COPIED_TWICE.contains(&path.as_str()) {
                size.saturating_mul(2)
            } else {
                *size
            }
        })
        .fold(0, u64::saturating_add);
    Ok(Some(bytes))
}

/// The app-group domain `source` names.
fn domain(source: &WhatsappConfig) -> &'static str {
    if source.business {
        BUSINESS_DOMAIN
    } else {
        DOMAIN
    }
}

/// Decrypt WhatsApp's files into `work` when `source.backup` is an
/// encrypted iPhone backup. `None` means the backup is not encrypted (or is
/// not a backup directory) and wtsexporter reads it as it is.
///
/// # Errors
///
/// Returns an error when the backup is encrypted and no password was given,
/// a password was given for a backup that is not encrypted, the password is
/// wrong, the disk that holds `work` cannot hold WhatsApp's files, or the
/// backup does not hold WhatsApp.
pub(crate) fn decrypt_if_encrypted(
    source: &WhatsappConfig,
    work: &Path,
    config: &ExporterConfig,
) -> Result<Option<DecryptedWhatsapp>> {
    decrypt_with(source, work, |backup, password, domain| {
        config.emit_log("The iPhone backup is encrypted; decrypting WhatsApp's files...");
        decrypt_ios_backup_domain(
            backup,
            password,
            domain,
            work,
            // The decrypted files go under the Scratch Directory, so its
            // disk is checked for room before the first one is written.
            |bytes| check_headroom(work, bytes, Disk::Scratch),
            config.log.clone(),
            config.progress.clone(),
        )
    })
}

/// [`decrypt_if_encrypted`] with the decryption itself passed in, so the
/// rules around it can be tested without an encrypted backup.
fn decrypt_with(
    source: &WhatsappConfig,
    work: &Path,
    decrypt: impl FnOnce(&Path, &str, &str) -> Result<DecryptedDomain>,
) -> Result<Option<DecryptedWhatsapp>> {
    let Some(backup) = source.backup.as_deref() else {
        return Ok(None);
    };
    let password = source.backup_password.as_deref();
    match (ios_backup_encrypted_flag(backup), password) {
        (Some(true), None) => bail!("The backup is encrypted — fill Encryption password."),
        (Some(false), Some(_)) => {
            bail!("This backup is not encrypted. Clear Encryption password.")
        }
        (Some(true), Some(password)) => {
            let domain = domain(source);
            decrypt(backup, password, domain)?;
            let domain_dir = work.join(domain);
            let database = domain_dir.join("ChatStorage.sqlite");
            if !database.is_file() {
                bail!(
                    "{} is not in this iPhone backup. {}",
                    if source.business {
                        "WhatsApp Business"
                    } else {
                        "WhatsApp"
                    },
                    if source.business {
                        "If the phone has WhatsApp, clear WhatsApp Business."
                    } else {
                        "If the phone has WhatsApp Business, tick WhatsApp Business."
                    }
                );
            }
            Ok(Some(DecryptedWhatsapp {
                domain_dir,
                database,
            }))
        }
        // Not encrypted, or not a directory with a readable Manifest.plist:
        // wtsexporter reads it, or says what is wrong with it.
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::{BUSINESS_DOMAIN, DOMAIN, DecryptedWhatsapp, decrypt_with, extract_bytes};
    use ios_backup::DecryptedDomain;
    use message_crate_core::WhatsappConfig;
    use std::fs;
    use std::path::Path;
    use tempfile::{TempDir, tempdir};

    /// A backup directory whose `Manifest.plist` says `encrypted`.
    fn backup(encrypted: bool) -> TempDir {
        let dir = tempdir().unwrap();
        let mut dict = plist::Dictionary::new();
        dict.insert("IsEncrypted".into(), plist::Value::Boolean(encrypted));
        plist::Value::Dictionary(dict)
            .to_file_xml(dir.path().join("Manifest.plist"))
            .unwrap();
        dir
    }

    fn source(backup: &Path, password: Option<&str>) -> WhatsappConfig {
        WhatsappConfig {
            backup: Some(backup.to_path_buf()),
            backup_password: password.map(str::to_string),
            ..WhatsappConfig::default()
        }
    }

    fn never(_: &Path, _: &str, _: &str) -> anyhow::Result<DecryptedDomain> {
        panic!("nothing should be decrypted");
    }

    /// An unencrypted backup is left for wtsexporter to read as it is.
    #[test]
    fn an_unencrypted_backup_is_not_decrypted() {
        let backup = backup(false);
        let work = tempdir().unwrap();
        let found = decrypt_with(&source(backup.path(), None), work.path(), never).unwrap();
        assert_eq!(found, None);
    }

    /// The form requires the password; a caller that skipped the form is
    /// told the same thing rather than left to a wtsexporter prompt that
    /// nobody can answer.
    #[test]
    fn an_encrypted_backup_without_a_password_is_refused() {
        let backup = backup(true);
        let work = tempdir().unwrap();
        let err = decrypt_with(&source(backup.path(), None), work.path(), never).unwrap_err();
        assert_eq!(
            err.to_string(),
            "The backup is encrypted — fill Encryption password."
        );
    }

    #[test]
    fn a_password_for_an_unencrypted_backup_is_refused() {
        let backup = backup(false);
        let work = tempdir().unwrap();
        let err =
            decrypt_with(&source(backup.path(), Some("secret")), work.path(), never).unwrap_err();
        assert_eq!(
            err.to_string(),
            "This backup is not encrypted. Clear Encryption password."
        );
    }

    /// The password and the app's own domain go to the decryption, and the
    /// answer names the files it left in the work directory.
    #[test]
    fn an_encrypted_backup_is_decrypted_into_the_work_directory() {
        let backup = backup(true);
        let work = tempdir().unwrap();
        for (business, expected_domain) in [(false, DOMAIN), (true, BUSINESS_DOMAIN)] {
            let source = WhatsappConfig {
                business,
                ..source(backup.path(), Some("secret"))
            };
            let found = decrypt_with(&source, work.path(), |from, password, domain| {
                assert_eq!(from, backup.path());
                assert_eq!(password, "secret");
                assert_eq!(domain, expected_domain);
                let dir = work.path().join(domain);
                fs::create_dir_all(&dir).unwrap();
                fs::write(dir.join("ChatStorage.sqlite"), b"db").unwrap();
                Ok(DecryptedDomain {
                    files: 1,
                    failures: 0,
                })
            })
            .unwrap();
            let domain_dir = work.path().join(expected_domain);
            assert_eq!(
                found,
                Some(DecryptedWhatsapp {
                    database: domain_dir.join("ChatStorage.sqlite"),
                    domain_dir,
                })
            );
        }
    }

    /// A backup of a phone with only the other WhatsApp app decrypts to
    /// nothing; the error names the checkbox that picks the app.
    #[test]
    fn a_backup_without_whatsapp_names_the_business_checkbox() {
        let backup = backup(true);
        let work = tempdir().unwrap();
        let err = decrypt_with(
            &source(backup.path(), Some("secret")),
            work.path(),
            |_, _, _| {
                Ok(DecryptedDomain {
                    files: 0,
                    failures: 0,
                })
            },
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "WhatsApp is not in this iPhone backup. \
             If the phone has WhatsApp Business, tick WhatsApp Business."
        );
    }

    /// A backup that is not encrypted, holding `files` (domain, path inside
    /// it, size) at the places its plain `Manifest.db` names.
    fn unencrypted_backup(files: &[(&str, &str, usize)]) -> TempDir {
        let dir = backup(false);
        let files: Vec<_> = files
            .iter()
            .map(|&(domain, path, size)| (domain, path, Some(size)))
            .collect();
        ios_backup::manifest_fixture::write_unencrypted_manifest(dir.path(), &files);
        dir
    }

    /// wtsexporter extracts every file of the domain from a backup that is
    /// not encrypted, and copies the three databases beside the extract as
    /// well, so those count twice. The Business app's domain is measured
    /// when Business is ticked, and another domain's file never (#1651).
    #[test]
    fn the_extract_of_an_unencrypted_backup_is_measured_before_it_starts() {
        let backup = unencrypted_backup(&[
            (DOMAIN, "ChatStorage.sqlite", 30),
            (DOMAIN, "ContactsV2.sqlite", 5),
            (DOMAIN, "CallHistory.sqlite", 2),
            (DOMAIN, "Message/Media/photo.jpg", 700),
            (BUSINESS_DOMAIN, "ChatStorage.sqlite", 40),
            ("HomeDomain", "Library/SMS/sms.db", 9),
        ]);

        let personal = source(backup.path(), None);
        assert_eq!(
            extract_bytes(&personal).unwrap(),
            Some(30 * 2 + 5 * 2 + 2 * 2 + 700)
        );
        let business = WhatsappConfig {
            business: true,
            ..personal
        };
        assert_eq!(extract_bytes(&business).unwrap(), Some(40 * 2));
    }

    /// An encrypted backup's files are measured as they are decrypted, and
    /// a run that names no backup extracts nothing, so neither is measured
    /// here.
    #[test]
    fn only_an_unencrypted_backup_has_an_extract_to_measure() {
        let encrypted = backup(true);
        assert_eq!(
            extract_bytes(&source(encrypted.path(), Some("secret"))).unwrap(),
            None
        );
        assert_eq!(extract_bytes(&WhatsappConfig::default()).unwrap(), None);
    }
}
