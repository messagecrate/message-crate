//! The file of each attachment an exporter's documents keep, for an
//! exporter that reads its attachments from files on disk.

use crate::write_queue::AttachmentSource;
use ir::Attachment;
use std::collections::VecDeque;
use std::path::PathBuf;

/// The file of each attachment of the documents projected so far, in the
/// order the writer asks for them ([`Project::attachment_source`]), or
/// `None` for an attachment with no file.
///
/// [`Project::attachment_source`]: crate::Project::attachment_source
#[derive(Debug, Default)]
pub struct AttachmentPaths(VecDeque<Option<PathBuf>>);

impl AttachmentPaths {
    /// Add the file of the next attachment, or `None` when it has none.
    pub fn push(&mut self, path: Option<PathBuf>) {
        self.0.push_back(path);
    }

    /// The next attachment's source and its size for the progress totals
    /// and the headroom check. An attachment that carries no size is given
    /// its file's size on disk, so a file whose rows give no size is still
    /// counted (#3057). An attachment with no file is `Missing`.
    pub fn next_source(&mut self, attachment: &Attachment) -> (AttachmentSource, Option<u64>) {
        match self.0.pop_front().flatten() {
            Some(path) => {
                let size = attachment
                    .size_bytes
                    .or_else(|| std::fs::metadata(&path).ok().map(|m| m.len()));
                (AttachmentSource::Path(path), size)
            }
            None => (AttachmentSource::Missing, attachment.size_bytes),
        }
    }
}

impl Extend<Option<PathBuf>> for AttachmentPaths {
    fn extend<I: IntoIterator<Item = Option<PathBuf>>>(&mut self, paths: I) {
        self.0.extend(paths);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(size_bytes: Option<u64>) -> Attachment {
        Attachment {
            path: None,
            original_name: None,
            mime_type: None,
            digest_sha256: None,
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes,
            missing_reason: None,
            bytes: None,
        }
    }

    #[test]
    fn an_attachment_with_no_size_is_given_its_file_size() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("photo.jpg");
        std::fs::write(&file, [0u8; 40]).unwrap();
        let mut paths = AttachmentPaths::default();
        paths.push(Some(file.clone()));
        let (source, size) = paths.next_source(&attachment(None));
        assert!(matches!(source, AttachmentSource::Path(path) if path == file));
        assert_eq!(size, Some(40));
    }

    #[test]
    fn an_attachment_size_is_kept_over_the_file_size() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("photo.jpg");
        std::fs::write(&file, [0u8; 40]).unwrap();
        let mut paths = AttachmentPaths::default();
        paths.push(Some(file));
        assert_eq!(paths.next_source(&attachment(Some(7))).1, Some(7));
    }

    #[test]
    fn attachments_take_their_files_in_order_and_one_with_none_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.jpg");
        std::fs::write(&first, [0u8; 3]).unwrap();
        let mut paths = AttachmentPaths::default();
        paths.extend([None, Some(first.clone())]);
        let (source, size) = paths.next_source(&attachment(Some(5)));
        assert!(matches!(source, AttachmentSource::Missing));
        assert_eq!(size, Some(5));
        let (source, _) = paths.next_source(&attachment(None));
        assert!(matches!(source, AttachmentSource::Path(path) if path == first));
        assert!(matches!(
            paths.next_source(&attachment(None)).0,
            AttachmentSource::Missing
        ));
    }
}
