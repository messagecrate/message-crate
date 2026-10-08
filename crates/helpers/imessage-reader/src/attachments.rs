//! Where an attachment's bytes are, and decrypting one out of an encrypted
//! backup when the app asks.

use std::path::{Path, PathBuf};

use crabapple::error::BackupError;
use imessage_database::tables::attachment::Attachment;
use imessage_reader_protocol::{AttachmentFile, Event};

use crate::{backup::decrypt_file, error::RuntimeError, session::MailSession};

/// The path Messages resolves for this attachment on the selected platform,
/// or `None` when it has no file.
pub(crate) fn resolved_path(session: &MailSession, attachment: &Attachment) -> Option<PathBuf> {
    attachment
        .resolved_attachment_path(
            &session.options.platform,
            &session.options.db_path,
            session.options.attachment_root.as_deref(),
        )
        .map(PathBuf::from)
}

/// Answer one attachment request: for an encrypted backup, decrypt the entry
/// into the scratch directory and name the file; otherwise name the path itself
/// when it exists.
///
/// An entry the backup does not hold is [`AttachmentFile::Missing`]: the
/// app records the attachment as missing and moves on, the same as it does
/// for a plain file that is gone, and the reason is on the log so a run's
/// worth of gaps does not go unexplained. Any other error, such as a scratch
/// directory with no room left, is [`AttachmentFile::Failed`] with the error,
/// which the app counts and reports on its own.
pub(crate) fn decrypt_for_app(session: &MailSession, source: &Path) -> Event {
    let Some(backup) = session
        .data_source
        .backup
        .as_ref()
        .filter(|b| b.is_encrypted())
    else {
        return Event::Attachment(if source.is_file() {
            AttachmentFile::Ready {
                path: source.to_path_buf(),
            }
        } else {
            AttachmentFile::Missing
        });
    };
    Event::Attachment(
        match decrypt_file(backup, source, session.options.scratch_dir()) {
            Ok(temp) => AttachmentFile::Ready { path: temp },
            Err(RuntimeError::BackupError(BackupError::FileNotFoundInBackup(_))) => {
                session.options.emit_log(format!(
                    "Attachment {} is not in the encrypted backup, so it is recorded without its file",
                    source.display()
                ));
                AttachmentFile::Missing
            }
            Err(e) => AttachmentFile::Failed {
                reason: e.to_string(),
            },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FixtureBackup, FixtureDb};
    use chat_db_fixture::{
        PHOTO_BYTES,
        ios_backup::{BACKUP_PASSWORD, Encryption, MEDIA_DOMAIN, PHOTO_PATH, stored_path},
    };

    /// The path the reader resolves for the photo of a session over
    /// `fixture`'s backup: the photo's stored file in the backup.
    fn photo_path(session: &MailSession, fixture: &FixtureBackup) -> PathBuf {
        let messages = FixtureDb::messages(session);
        let attachments = Attachment::from_message(session.data_source.db(), &messages[0]).unwrap();
        let path = resolved_path(session, &attachments[0]).unwrap();
        assert_eq!(
            path,
            stored_path(fixture.backup.path(), MEDIA_DOMAIN, PHOTO_PATH)
        );
        path
    }

    /// An encrypted backup's photo decrypts to its original bytes, into a
    /// file of its own in the request's scratch directory (#788).
    #[test]
    fn an_encrypted_backups_photo_decrypts_to_its_bytes_in_the_scratch_directory() {
        let fixture = FixtureBackup::write(Encryption::Password(BACKUP_PASSWORD));
        let session = MailSession::new(fixture.options(Some(BACKUP_PASSWORD))).unwrap();
        let photo = photo_path(&session, &fixture);

        let Event::Attachment(AttachmentFile::Ready { path }) = decrypt_for_app(&session, &photo)
        else {
            panic!("the photo was not decrypted");
        };
        assert_eq!(path.parent(), Some(fixture.scratch.path()));
        assert_eq!(std::fs::read(&path).unwrap(), PHOTO_BYTES);
        assert_ne!(std::fs::read(&photo).unwrap(), PHOTO_BYTES);
    }

    /// A backup that is not encrypted holds the photo as it is, so the
    /// answer names the stored file itself and nothing is decrypted.
    #[test]
    fn an_unencrypted_backups_photo_is_named_where_it_is() {
        let fixture = FixtureBackup::write(Encryption::None);
        let session = MailSession::new(fixture.options(None)).unwrap();
        let photo = photo_path(&session, &fixture);

        assert!(matches!(
            decrypt_for_app(&session, &photo),
            Event::Attachment(AttachmentFile::Ready { path }) if path == photo
        ));
        assert_eq!(std::fs::read(&photo).unwrap(), PHOTO_BYTES);
        assert!(fixture.scratch_files().is_empty());
    }

    /// On a Mac the row's `filename` is the path; a row without one has no
    /// file.
    #[test]
    fn a_mac_attachment_resolves_to_the_path_the_row_names() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let messages = FixtureDb::messages(&session);
        let mut attachments =
            Attachment::from_message(session.data_source.db(), &messages[0]).unwrap();
        assert_eq!(attachments.len(), 1);

        assert_eq!(
            resolved_path(&session, &attachments[0]),
            Some(fixture.dir.path().join("photo.jpg"))
        );

        attachments[0].filename = None;
        assert_eq!(resolved_path(&session, &attachments[0]), None);
    }

    /// Without an encrypted backup there is nothing to decrypt: the answer
    /// names the file when it exists and nothing when it does not.
    #[test]
    fn an_unencrypted_source_answers_with_the_path_itself() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let photo = fixture.dir.path().join("photo.jpg");

        assert!(matches!(
            decrypt_for_app(&session, &photo),
            Event::Attachment(AttachmentFile::Ready { path }) if path == photo
        ));
        assert!(matches!(
            decrypt_for_app(&session, &fixture.dir.path().join("gone.jpg")),
            Event::Attachment(AttachmentFile::Missing)
        ));
    }
}
