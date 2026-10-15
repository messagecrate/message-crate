//! What a run does to its documents on the way out: whether it copies the
//! attachment files, obfuscation, and the sinks the write tail reports
//! through.

use crate::config::{AttachmentFiles, OutputConfig, RunContext};
use crate::process::LogSink;
use crate::progress::ProgressSink;

/// The transforms a run applies at write time, handed to the format sink.
#[derive(Debug, Clone)]
pub struct ExportTransforms {
    /// Whether the run copies the original attachment files.
    pub media: AttachmentFiles,
    /// Whether to replace PII and media with obfuscated placeholders.
    pub obfuscate: bool,
    /// Seed for deterministic obfuscation; `None` generates one.
    pub obfuscate_seed: Option<String>,
    /// Mid-run notes (e.g. generated obfuscate seed).
    pub log: LogSink,
    /// Typed progress events for the write tail.
    pub progress: ProgressSink,
}

impl Default for ExportTransforms {
    fn default() -> Self {
        Self {
            media: AttachmentFiles::Originals,
            obfuscate: false,
            obfuscate_seed: None,
            log: LogSink::none(),
            progress: ProgressSink::none(),
        }
    }
}

impl ExportTransforms {
    /// The transforms an exporter run wants: media and obfuscation from
    /// `output` (obfuscation is on when either its flag or a seed is set),
    /// plus the run's log and progress sinks from `context` so the write tail
    /// reports through the same hooks the exporter does.
    pub fn new(context: &RunContext, output: &OutputConfig) -> Self {
        Self {
            media: output.media.mode,
            obfuscate: output.obfuscate_active(),
            obfuscate_seed: output.obfuscate.seed.clone(),
            log: context.log.clone(),
            progress: context.progress.clone(),
        }
    }

    /// All-defaults transform set (copy the originals, no obfuscation, sinks
    /// that drop what they get).
    pub fn none() -> Self {
        Self::default()
    }

    /// True when attachment bytes should be staged under `attachments/`
    /// (false when obfuscating).
    pub fn copies_attachments(&self) -> bool {
        // Obfuscate discards real bytes; skip staging them in the first place.
        !self.obfuscate && self.media.copies()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MediaConfig, ObfuscateConfig, OutputFormat};
    use crate::progress::ProgressEvent;
    use std::sync::{Arc, Mutex};

    fn output(obfuscate: ObfuscateConfig, media: MediaConfig) -> OutputConfig {
        OutputConfig {
            obfuscate,
            media,
            ..OutputConfig::new("out", OutputFormat::Json)
        }
    }

    /// The transforms of a run that writes `output`, reporting nowhere.
    fn transforms(output: &OutputConfig) -> ExportTransforms {
        ExportTransforms::new(&RunContext::new("/cache"), output)
    }

    #[test]
    fn new_obfuscates_when_the_flag_is_set_without_a_seed() {
        let t = transforms(&output(
            ObfuscateConfig {
                enabled: true,
                seed: None,
            },
            MediaConfig::default(),
        ));
        assert!(t.obfuscate);
        assert_eq!(t.obfuscate_seed, None);
    }

    #[test]
    fn new_obfuscates_when_only_a_seed_is_given() {
        let t = transforms(&output(
            ObfuscateConfig {
                enabled: false,
                seed: Some("abc123".to_string()),
            },
            MediaConfig::default(),
        ));
        assert!(t.obfuscate);
        assert_eq!(t.obfuscate_seed.as_deref(), Some("abc123"));
    }

    #[test]
    fn new_leaves_obfuscation_off_without_flag_or_seed() {
        let t = transforms(&output(ObfuscateConfig::default(), MediaConfig::default()));
        assert!(!t.obfuscate);
    }

    #[test]
    fn new_carries_the_attachment_choice_and_sinks() {
        let out = output(
            ObfuscateConfig::default(),
            MediaConfig {
                mode: AttachmentFiles::Skip,
                ..MediaConfig::default()
            },
        );
        let mut context = RunContext::new("/cache");
        let lines = Arc::new(Mutex::new(Vec::new()));
        let lines_seen = Arc::clone(&lines);
        context.log = LogSink::new(move |line| lines_seen.lock().unwrap().push(line.to_string()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_seen = Arc::clone(&events);
        context.progress =
            ProgressSink::unpaced(move |event| events_seen.lock().unwrap().push(event));

        let t = ExportTransforms::new(&context, &out);
        t.log.emit("a line");
        t.progress.emit(ProgressEvent::Parse { done: 1, total: 2 });

        assert_eq!(t.media, AttachmentFiles::Skip);
        assert_eq!(lines.lock().unwrap().as_slice(), ["a line"]);
        assert_eq!(events.lock().unwrap().len(), 1);
    }

    #[test]
    fn obfuscate_disables_copy() {
        let t = ExportTransforms {
            media: AttachmentFiles::Originals,
            obfuscate: true,
            ..ExportTransforms::none()
        };
        assert!(!t.copies_attachments());

        let t = ExportTransforms {
            media: AttachmentFiles::Originals,
            obfuscate: false,
            ..ExportTransforms::none()
        };
        assert!(t.copies_attachments());
    }
}
