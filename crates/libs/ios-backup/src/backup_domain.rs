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
use message_crate_core::Sinks;

use crate::helper::{Helper, locate};

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
/// the domain. Log lines and progress counts go to `sinks`. Setting
/// `sinks.cancel` kills the `imessage-reader` program, even while the
/// program is decrypting and sends nothing (#3034).
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
/// `check` refuses. Returns [`Canceled`](message_crate_core::Canceled) when
/// the cancel flag is set, having killed the program.
pub fn decrypt_ios_backup_domain(
    backup_root: &Path,
    backup_password: &str,
    domain: &str,
    out_dir: &Path,
    check: impl FnOnce(u64) -> Result<()>,
    sinks: Sinks<'_>,
) -> Result<DecryptedDomain> {
    let request = Request::BackupDomain(BackupDomainRequest {
        backup_path: backup_root.to_path_buf(),
        backup_password: backup_password.to_string(),
        domain: domain.to_string(),
        out_dir: out_dir.to_path_buf(),
    });
    decrypt_with(&locate()?, &request, check, sinks)
}

/// [`decrypt_ios_backup_domain`] with the program at `program`.
fn decrypt_with(
    program: &Path,
    request: &Request,
    check: impl FnOnce(u64) -> Result<()>,
    sinks: Sinks<'_>,
) -> Result<DecryptedDomain> {
    // Cancel kills the program even while it decrypts and sends nothing.
    let helper = Helper::spawn_at(program, request, sinks.log.clone(), sinks.progress.clone())?
        .cancel_on(sinks.cancel.clone());
    read_answer(helper, check)
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

    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use message_crate_core::{CancelFlag, Canceled, LogSink, ProgressSink, Sinks};

    use super::{DecryptedDomain, decrypt_with, read_answer};
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

    /// Cancel is set when the program logs that it is decrypting, and the
    /// program then sends nothing more, as a long decrypt of a large domain
    /// does. The decrypt must return [`Canceled`] rather than wait for a
    /// line that never comes (#3034).
    #[test]
    fn cancel_stops_a_decrypt_the_program_never_answers() {
        let dir = tempfile::tempdir().unwrap();
        // `exec` puts `sleep` in the shell's place, so the process the
        // helper started is the one that goes quiet.
        let body = format!(
            "{}\n\
             echo '{{\"event\":\"backup_domain_size\",\"files\":2,\"bytes\":300}}'\n\
             read -r go\n\
             echo '{{\"event\":\"log\",\"line\":\"decrypting\"}}'\n\
             exec sleep 100000",
            source_line(PROTOCOL_VERSION)
        );
        let program = fake_helper(dir.path(), &body);
        let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
        let log = {
            let cancel = Arc::clone(&cancel);
            LogSink::new(move |line| {
                if line == "decrypting" {
                    cancel.store(true, Ordering::Relaxed);
                }
            })
        };
        let sinks = Sinks {
            log: &log,
            progress: &ProgressSink::none(),
            cancel: &cancel,
        };
        let err = decrypt_with(&program, &request(), |_| Ok(()), sinks).unwrap_err();
        assert!(err.downcast_ref::<Canceled>().is_some(), "{err:#}");
    }
}
