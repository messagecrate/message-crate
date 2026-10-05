//! Have `imessage-reader` decrypt one domain of an encrypted iPhone backup.
//!
//! The WhatsApp importer calls this before it runs wtsexporter: wtsexporter
//! asks for a backup password on a terminal and takes it no other way, and
//! the only code in the repository that can decrypt an iPhone backup is the
//! GPL `crabapple`, which stays behind the helper's process boundary
//! (`docs/adr/0014-gpl-code-only-behind-a-process-boundary.md`).

use std::path::Path;

use anyhow::{Result, bail};
use imessage_reader_protocol::{BackupDomainRequest, Event, Request};
use message_crate_core::{LogSink, ProgressSink};

use crate::helper::Helper;

/// What [`decrypt_ios_backup_domain`] wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecryptedDomain {
    /// Files written. Zero means the backup does not hold the domain.
    pub files: u64,
    /// Files the backup lists that could not be decrypted; each has a line
    /// on the log.
    pub failures: u64,
}

/// Decrypt every file of `domain` in the encrypted iPhone backup at
/// `backup_root` into `<out_dir>/<domain>/`, keeping each file's path inside
/// the domain. Log lines and progress counts go to the given sinks.
///
/// Before anything is written, `check` gets the bytes the domain's files
/// hold, as the backup's manifest records them, so the caller can refuse a
/// disk that cannot hold them. An error from `check` stops the program with
/// nothing written and is returned as it is.
///
/// # Errors
///
/// Returns an error when the directory is not an encrypted iPhone backup, the
/// password is wrong, there is no `imessage-reader` program to run, or
/// `check` refuses.
pub fn decrypt_ios_backup_domain(
    backup_root: &Path,
    backup_password: &str,
    domain: &str,
    out_dir: &Path,
    check: impl FnOnce(u64) -> Result<()>,
    log: Option<LogSink>,
    progress: Option<ProgressSink>,
) -> Result<DecryptedDomain> {
    let request = Request::BackupDomain(BackupDomainRequest {
        backup_path: backup_root.to_path_buf(),
        backup_password: backup_password.to_string(),
        domain: domain.to_string(),
        out_dir: out_dir.to_path_buf(),
    });
    read_answer(Helper::spawn(&request, log, progress)?, check)
}

/// The counts a started program answers the request with, once `check` has
/// passed the size it measured.
fn read_answer(
    mut helper: Helper,
    check: impl FnOnce(u64) -> Result<()>,
) -> Result<DecryptedDomain> {
    let mut check = Some(check);
    let answer = loop {
        match helper.next_event()? {
            Event::Source { .. } => {}
            Event::BackupDomainSize { bytes, .. } => {
                let Some(check) = check.take() else {
                    bail!("imessage-reader measured the domain twice");
                };
                if let Err(refused) = check(bytes) {
                    // Closing stdin without the go stops the program before
                    // it writes anything.
                    helper.finish()?;
                    return Err(refused);
                }
                helper.send(&Request::DecryptDomain)?;
            }
            Event::BackupDomainDone { .. } if check.is_some() => {
                bail!("imessage-reader decrypted the domain before measuring it");
            }
            Event::BackupDomainDone { files, failures } => {
                break DecryptedDomain { files, failures };
            }
            other => bail!("expected the decrypted-domain answer, got {other:?}"),
        }
    };
    helper.finish()?;
    Ok(answer)
}

#[cfg(all(test, unix))]
mod tests {
    use imessage_reader_protocol::{BackupDomainRequest, PROTOCOL_VERSION, Request};

    use super::{DecryptedDomain, read_answer};
    use crate::testutil::{fake_helper, source_line, spawn_fake};

    fn request() -> Request {
        Request::BackupDomain(BackupDomainRequest {
            backup_path: "/nowhere/backup".into(),
            backup_password: "secret".into(),
            domain: "AppDomainGroup-group.net.whatsapp.WhatsApp.shared".into(),
            out_dir: "/nowhere/out".into(),
        })
    }

    /// A fake that measures the domain at 300 bytes, then decrypts only
    /// when the next line on its stdin is the go, leaving `went` in `dir`
    /// when it read one.
    fn measuring_fake(dir: &std::path::Path) -> std::path::PathBuf {
        let went = dir.join("went");
        let body = format!(
            "{}\n\
             echo '{{\"event\":\"backup_domain_size\",\"files\":2,\"bytes\":300}}'\n\
             if read -r go; then\n\
               echo \"$go\" > '{}'\n\
               echo '{{\"event\":\"backup_domain_done\",\"files\":12,\"failures\":1}}'\n\
             fi",
            source_line(PROTOCOL_VERSION),
            went.display()
        );
        fake_helper(dir, &body)
    }

    /// The size goes to the check before anything is decrypted, and a check
    /// that passes sends the go and gets the counts.
    #[test]
    fn the_counts_follow_the_check_of_the_measured_size() {
        let dir = tempfile::tempdir().unwrap();
        let helper = spawn_fake(&measuring_fake(dir.path()), &request());
        let mut measured = None;
        let answer = read_answer(helper, |bytes| {
            measured = Some(bytes);
            Ok(())
        })
        .unwrap();
        assert_eq!(measured, Some(300));
        assert_eq!(
            answer,
            DecryptedDomain {
                files: 12,
                failures: 1
            }
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("went")).unwrap(),
            "{\"op\":\"decrypt_domain\"}\n"
        );
    }

    /// A check that refuses is returned as it is, and the program is never
    /// told to go, so it decrypts nothing (#1651).
    #[test]
    fn a_refused_check_stops_the_program_before_it_decrypts() {
        let dir = tempfile::tempdir().unwrap();
        let helper = spawn_fake(&measuring_fake(dir.path()), &request());
        let err = read_answer(helper, |_| anyhow::bail!("no room")).unwrap_err();
        assert_eq!(err.to_string(), "no room");
        assert!(!dir.path().join("went").exists());
    }
}
