//! Report types written at the end of an Upload, plus the small formatting
//! helpers that turn them into log text.
//!
//! [`PushReport`] is the JSON file left next to the export and the payload of
//! [`crate::ProgressEvent::Finished`]. Everything here is plain data with no
//! I/O so the desktop app and tests can build and inspect reports directly.

use message_crate_api_types::ImportMode;
use message_crate_core::count_of;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Per-conversation outcome written into the final report JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileResult {
    /// File name relative to the input directory.
    pub file: String,
    /// `ok`, `failed`, `skipped`, or `cancelled` (the run was stopped before
    /// every message of the file was sent).
    pub status: String,
    /// The failure, when `status` is `failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Messages of this file the server accepted.
    pub messages: u64,
    /// Attachments this file refers to, uploaded or not.
    pub attachments: u64,
    /// Timings and sizes, for a file that was read and prepared. Absent for a
    /// file the journal skipped or one that failed before preparing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<UploadProfile>,
}

impl FileResult {
    /// Result row for a conversation the journal says was already imported.
    pub(crate) fn skipped(file: &str) -> Self {
        Self {
            file: file.to_string(),
            status: "skipped".into(),
            error: None,
            messages: 0,
            attachments: 0,
            profile: None,
        }
    }

    /// Result row for a conversation the run was stopped before taking up.
    pub(crate) fn cancelled(file: &str) -> Self {
        Self {
            file: file.to_string(),
            status: "cancelled".into(),
            error: None,
            messages: 0,
            attachments: 0,
            profile: None,
        }
    }

    /// Result row for a conversation that failed before any message was queued.
    pub(crate) fn failed(file: &str, error: &str) -> Self {
        Self {
            file: file.to_string(),
            status: "failed".into(),
            error: Some(error.to_string()),
            messages: 0,
            attachments: 0,
            profile: None,
        }
    }
}

/// Timing and size stats for one conversation (used for the log line that
/// says where its time went).
///
/// These numbers help answer "why was this chat slow?" — reading JSON Lines,
/// hashing/scanning attachments, uploading media, or importing messages.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadProfile {
    /// Reading and parsing the JSON Lines file.
    pub read_ms: u64,
    /// Finding attachments on disk and hashing them.
    pub attachment_scan_hash_ms: u64,
    /// Uploading attachment bytes.
    pub asset_upload_ms: u64,
    /// Posting the message batches.
    pub message_import_ms: u64,
    /// The whole file, start to finish.
    pub total_ms: u64,
    /// Distinct attachments by fingerprint.
    pub unique_assets: u64,
    /// Bytes across those distinct attachments.
    pub asset_bytes: u64,
}

/// Final summary of a whole Upload (also written to disk as the report file).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PushReport {
    /// `true` when no conversation failed and the run was not cancelled.
    pub ok: bool,
    /// `true` when the run halted before it finished, so the Upload is paused.
    /// The cancel flag halts it when it leaves conversations unsent: they are
    /// in `conversations_cancelled`, and the journal leaves them for the next
    /// Upload. A cancel that arrives after the last request went out halts
    /// nothing, so it is `false` then. Only the cancel flag halts a run: a
    /// failed request fails its conversations and the run goes on. A session
    /// the server refused mid-run halts the run through the same flag, and
    /// it is `true` then even when no conversation was left unsent.
    pub cancelled: bool,
    /// `true` when the server refused the session token during the run (it
    /// expired or was ended), which halted the run as for a cancel. The
    /// requests it refused fail no conversation. The caller ends the session
    /// on its side as well, since every later request would be refused.
    pub session_refused: bool,
    /// `true` when the server refused to complete the Import Run this Upload
    /// started, so it still holds the run as running and `ok` is `false`.
    /// Only a run that halted (`cancelled`) is returned with this set: the
    /// caller tells a pause from a failure by `cancelled`, so the report
    /// reaches it and [`crate::ProgressEvent::Finished`] fires. A refused
    /// completion of a run that did not halt is the error `run` returns
    /// instead, and the report on disk carries the flag. Always `false` when
    /// the caller handed the Upload its Import Run, since the caller then
    /// completes it.
    pub completion_refused: bool,
    /// Account id the token resolved to.
    pub account: i64,
    /// Username the server reports for that account.
    pub username: String,
    /// `append` or `replace`.
    pub mode: ImportMode,
    /// Unix seconds when the run started, as a string.
    pub started_at: String,
    /// Unix seconds when the run finished, as a string.
    pub finished_at: String,
    /// Wall-clock time from start of auth through the last import.
    pub elapsed_ms: u64,
    /// Conversation files the run found.
    pub conversations_total: u64,
    /// Files imported without error.
    pub conversations_ok: u64,
    /// Files that failed.
    pub conversations_failed: u64,
    /// Files the journal said were already imported.
    pub conversations_skipped: u64,
    /// Files the run was stopped before it finished sending.
    pub conversations_cancelled: u64,
    /// Messages placed in HTTP import request bodies.
    #[serde(default)]
    pub messages_attempted: u64,
    /// Messages the server inserted as new rows.
    #[serde(default)]
    pub messages_inserted: u64,
    /// Attempted messages the server reported as already present.
    #[serde(default)]
    pub messages_deduped: u64,
    /// Messages in HTTP requests that failed after all retries.
    #[serde(default)]
    pub messages_failed: u64,
    /// Attachments whose bytes went up this run.
    pub assets_uploaded: u64,
    /// Attachments whose bytes did not go up this run: the server or another
    /// conversation already had the fingerprint, the file was left out (no
    /// path, missing, too large), or the Upload was text-only.
    pub assets_skipped: u64,
    /// Bytes uploaded.
    pub assets_bytes: u64,
    /// One row per conversation file.
    pub results: Vec<FileResult>,
}

/// Running totals of messages attempted, inserted, already present, and failed.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MessageAccounting {
    pub attempted: u64,
    pub inserted: u64,
    pub deduped: u64,
    pub failed: u64,
}

/// Running totals of attachment uploads across every prepared conversation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AssetTotals {
    pub uploaded: u64,
    pub skipped: u64,
    pub bytes: u64,
}

impl AssetTotals {
    /// Add one conversation's upload counts onto the run totals.
    pub(crate) fn add(&mut self, other: AssetTotals) {
        self.uploaded = self.uploaded.saturating_add(other.uploaded);
        self.skipped = self.skipped.saturating_add(other.skipped);
        self.bytes = self.bytes.saturating_add(other.bytes);
    }
}

/// Totals derived from per-conversation [`FileResult`] rows.
#[derive(Debug, Default)]
pub(crate) struct FileResultCounts {
    pub ok: u64,
    pub failed: u64,
    pub skipped: u64,
    pub cancelled: u64,
}

/// Count ok / failed / skipped / cancelled conversations.
pub(crate) fn count_file_results(results: &[FileResult]) -> FileResultCounts {
    let mut counted = FileResultCounts::default();
    for result in results {
        match result.status.as_str() {
            "ok" => counted.ok += 1,
            "failed" => counted.failed += 1,
            "skipped" => counted.skipped += 1,
            "cancelled" => counted.cancelled += 1,
            _ => {}
        }
    }
    counted
}

/// Unix time in seconds as a string (for report timestamps).
pub(crate) fn now_stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or_else(|_| "0".into(), |d| d.as_secs().to_string())
}

/// Milliseconds since `started` (for the timing fields of [`UploadProfile`]).
pub(crate) fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Turn a millisecond count into a short human string like `34m12s` or `1h02m03s`.
pub fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}h{minutes:02}m{seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else if total_secs > 0 || ms == 0 {
        format!("{seconds}s")
    } else {
        format!("{ms}ms")
    }
}

/// Three-way Import Run status for `/v1/imports/{id}/complete`, read from the
/// Upload's report rather than from whether the Upload returned. `failed` has a
/// zero floor: cancelled (the run halted, see [`PushReport::cancelled`]), or
/// nothing landed at all. A skip-only repeat Upload is a no-op,
/// not a failure. Item-level failures beside successes are
/// `completed_with_issues`.
pub fn outcome_status(report: &PushReport) -> &'static str {
    let nothing_landed = report.conversations_total > 0
        && report.conversations_ok == 0
        && report.conversations_skipped == 0;
    if report.cancelled || nothing_landed {
        return "failed";
    }
    if report.conversations_failed > 0 || report.messages_failed > 0 {
        return "completed_with_issues";
    }
    "completed"
}

/// Format a millisecond count as seconds with one decimal place, like `3.3s`.
pub(crate) fn format_ms_seconds(ms: u64) -> String {
    format!("{:.1}s", ms as f64 / 1000.0)
}

/// Build the sentences that end the Upload's log: how the Upload ended, and
/// what it did with the conversations, messages and Assets it read.
pub fn format_push_summary(report: &PushReport) -> String {
    let elapsed = format_duration_ms(report.elapsed_ms);
    // A run that halted is paused (CONTEXT.md, Pause). A cancel that came
    // after the last conversation was sent halted nothing, so that run
    // completed.
    let ending = if report.cancelled {
        if report.session_refused {
            format!("The Upload paused after {elapsed}, because the server refused the session.")
        } else {
            format!("The Upload paused after {elapsed}.")
        }
    } else if report.conversations_failed > 0 || report.messages_failed > 0 {
        format!("The Upload completed in {elapsed}, with errors.")
    } else {
        format!("The Upload completed in {elapsed}.")
    };
    // `assets_skipped` counts attachments whose Asset did not go up: the
    // server already held it, its file had no path, was missing or was too
    // large, or the Upload sent text only.
    let not_uploaded = match report.assets_skipped {
        0 => String::new(),
        1 => " The Asset of 1 attachment was not uploaded, because the server already \
               held it, its file was missing or too large, or the Upload sent text only."
            .to_string(),
        n => format!(
            " The Assets of {n} attachments were not uploaded, because the server already \
             held them, their files were missing or too large, or the Upload sent text only."
        ),
    };
    format!(
        "{ending}\n\
Sent {} of {}, with {} failed, {} sent before, and {} left for the next Upload.\n\
The Upload tried to send {}: the server added {} and already held {}, \
and the requests for {} failed.\n\
Uploaded {}.{not_uploaded}",
        report.conversations_ok,
        count_of(report.conversations_total, "conversation", "conversations"),
        report.conversations_failed,
        report.conversations_skipped,
        report.conversations_cancelled,
        count_of(report.messages_attempted, "message", "messages"),
        report.messages_inserted,
        report.messages_deduped,
        report.messages_failed,
        count_of(report.assets_uploaded, "Asset", "Assets"),
    )
}

/// One sentence with the timings of each part of a conversation's work, so a
/// slow conversation shows where its time went.
pub(crate) fn format_profile_line(name: &str, profile: &UploadProfile) -> String {
    format!(
        "{name} took {} in all: {} reading the file, {} finding and hashing {}, \
         {} uploading {}, and {} importing messages.",
        format_ms_seconds(profile.total_ms),
        format_ms_seconds(profile.read_ms),
        format_ms_seconds(profile.attachment_scan_hash_ms),
        count_of(profile.unique_assets, "Asset", "Assets"),
        format_ms_seconds(profile.asset_upload_ms),
        media::format_bytes(profile.asset_bytes),
        format_ms_seconds(profile.message_import_ms),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_ms_humanizes() {
        assert_eq!(format_duration_ms(0), "0s");
        assert_eq!(format_duration_ms(999), "999ms");
        assert_eq!(format_duration_ms(12_000), "12s");
        assert_eq!(format_duration_ms(2_052_000), "34m12s");
        assert_eq!(format_duration_ms(3_723_000), "1h02m03s");
    }

    fn sample_report() -> PushReport {
        PushReport {
            ok: true,
            cancelled: false,
            session_refused: false,
            completion_refused: false,
            account: 1,
            username: "user".into(),
            mode: ImportMode::Append,
            started_at: "2026-08-29T00:00:00Z".into(),
            finished_at: "2026-08-29T00:01:00Z".into(),
            elapsed_ms: 60_000,
            conversations_total: 10,
            conversations_ok: 10,
            conversations_failed: 0,
            conversations_skipped: 0,
            conversations_cancelled: 0,
            messages_attempted: 100,
            messages_inserted: 90,
            messages_deduped: 10,
            messages_failed: 0,
            assets_uploaded: 5,
            assets_skipped: 0,
            assets_bytes: 1_000,
            results: Vec::new(),
        }
    }

    #[test]
    fn format_push_summary_writes_sentences() {
        let report = PushReport {
            ok: false,
            elapsed_ms: 12_000,
            conversations_ok: 8,
            conversations_failed: 1,
            conversations_skipped: 1,
            assets_uploaded: 4,
            assets_skipped: 2,
            assets_bytes: 1_048_576,
            ..sample_report()
        };
        assert_eq!(
            format_push_summary(&report),
            "The Upload completed in 12s, with errors.\n\
             Sent 8 of 10 conversations, with 1 failed, 1 sent before, \
             and 0 left for the next Upload.\n\
             The Upload tried to send 100 messages: the server added 90 and already held 10, \
             and the requests for 0 failed.\n\
             Uploaded 4 Assets. The Assets of 2 attachments were not uploaded, \
             because the server already held them, their files were missing or too large, \
             or the Upload sent text only."
        );
    }

    /// One of each is worded singular.
    #[test]
    fn format_push_summary_words_one_singular() {
        let one = PushReport {
            conversations_total: 1,
            conversations_ok: 1,
            messages_attempted: 1,
            messages_inserted: 1,
            messages_deduped: 0,
            assets_uploaded: 1,
            assets_skipped: 1,
            ..sample_report()
        };
        let summary = format_push_summary(&one);
        assert!(summary.contains("Sent 1 of 1 conversation, "), "{summary}");
        assert!(
            summary.contains("The Upload tried to send 1 message: "),
            "{summary}"
        );
        assert!(
            summary.contains(
                "Uploaded 1 Asset. The Asset of 1 attachment was not uploaded, because the \
                 server already held it, its file was missing or too large, or the Upload \
                 sent text only."
            ),
            "{summary}"
        );
    }

    /// The first line says how the Upload ended, from the report's fields: a
    /// run that left conversations for the next Upload is paused, and only
    /// such a run.
    #[test]
    fn format_push_summary_says_how_the_upload_ended() {
        let first_line = |report: PushReport| {
            format_push_summary(&report)
                .lines()
                .next()
                .unwrap()
                .to_string()
        };
        let not_ok = PushReport {
            ok: false,
            ..sample_report()
        };
        assert_eq!(
            first_line(PushReport {
                cancelled: true,
                conversations_cancelled: 6,
                ..not_ok.clone()
            }),
            "The Upload paused after 1m00s."
        );
        assert_eq!(
            first_line(PushReport {
                cancelled: true,
                session_refused: true,
                conversations_cancelled: 6,
                ..not_ok.clone()
            }),
            "The Upload paused after 1m00s, because the server refused the session."
        );
        // A refused session halts the Upload even when the batch it refused
        // carried only a conversation that had already failed.
        assert_eq!(
            first_line(PushReport {
                cancelled: true,
                session_refused: true,
                conversations_failed: 1,
                ..not_ok.clone()
            }),
            "The Upload paused after 1m00s, because the server refused the session."
        );
        assert_eq!(
            first_line(PushReport {
                conversations_failed: 1,
                ..not_ok
            }),
            "The Upload completed in 1m00s, with errors."
        );
    }

    #[test]
    fn the_profile_line_says_where_a_conversations_time_went() {
        let profile = UploadProfile {
            read_ms: 120,
            attachment_scan_hash_ms: 300,
            asset_upload_ms: 2_000,
            message_import_ms: 3_050,
            total_ms: 5_500,
            unique_assets: 3,
            asset_bytes: 2_048,
        };
        assert_eq!(
            format_profile_line("chat.jsonl", &profile),
            "chat.jsonl took 5.5s in all: 0.1s reading the file, 0.3s finding and hashing \
             3 Assets, 2.0s uploading 2.0 KB, and 3.0s importing messages."
        );
    }

    #[test]
    fn outcome_status_matches_the_spec_verdicts() {
        // Clean run.
        assert_eq!(outcome_status(&sample_report()), "completed");

        // Cancelled is failed regardless of counts.
        let mut cancelled = sample_report();
        cancelled.cancelled = true;
        assert_eq!(outcome_status(&cancelled), "failed");

        // Nothing landed at all: the zero floor.
        let mut nothing = sample_report();
        nothing.ok = false;
        nothing.conversations_ok = 0;
        nothing.conversations_failed = 10;
        assert_eq!(outcome_status(&nothing), "failed");

        // A skip-only repeat Upload is a no-op, not a failure.
        let mut skips = sample_report();
        skips.conversations_ok = 0;
        skips.conversations_skipped = 10;
        assert_eq!(outcome_status(&skips), "completed");

        // Item-level failures beside successes.
        let mut partial = sample_report();
        partial.ok = false;
        partial.conversations_ok = 8;
        partial.conversations_failed = 2;
        assert_eq!(outcome_status(&partial), "completed_with_issues");

        // Message failures inside ok conversations.
        let mut msgs = sample_report();
        msgs.messages_failed = 3;
        assert_eq!(outcome_status(&msgs), "completed_with_issues");
    }

    #[test]
    fn count_file_results_counts_each_status() {
        let results = vec![
            FileResult {
                file: "a.jsonl".into(),
                status: "ok".into(),
                error: None,
                messages: 5,
                attachments: 2,
                profile: None,
            },
            FileResult::failed("b.jsonl", "boom"),
            FileResult::skipped("c.jsonl"),
            FileResult::cancelled("d.jsonl"),
        ];
        let counted = count_file_results(&results);
        assert_eq!(
            (
                counted.ok,
                counted.failed,
                counted.skipped,
                counted.cancelled
            ),
            (1, 1, 1, 1)
        );
    }
}
