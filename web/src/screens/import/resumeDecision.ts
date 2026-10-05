import type { ActiveImportRun, SourceFingerprint } from "../../lib/importRun";
import type { PathStat } from "../../lib/tauri";

/** What entering Import should do about a session that already exists. */
export type ResumeDecision = {
  kind:
    | "none"
    | "other_device"
    | "directory_missing"
    // The run directory was recorded but the stat of it failed, so whether
    // it is there is not known. Distinct from directory_missing because an IPC
    // error is not evidence the directory is gone.
    | "directory_unknown"
    | "resume_upload"
    // A session waiting at either review: the summary is recomputed
    // fresh from the directory and shown again, nothing restored, because the
    // directory is the truth.
    | "resume_review"
    // A session that died mid media pass: the pass re-runs over whatever
    // originals it had not reached yet, which is safe because an original
    // still on disk always means work remains, then
    // continues to the Media Review exactly as the normal flow does.
    | "resume_media"
    // A session whose copy was interrupted: the exporter reads the backup
    // again and skips the conversations already written.
    | "resume_write"
    // The backup this session was reading is not the one on disk now, so
    // copying more of it into the same directory would mix two sources.
    | "source_changed"
    | "restart"
    // resumeDecisionFor never returns this: it has no way to know whether a
    // session's stored form snapshot is readable. The screen constructs it
    // itself when restoreFormFromSnapshot rejects the snapshot at the point
    // of trying to resume or restart.
    | "settings_unreadable";
  session: ActiveImportRun | null;
};

/** Whether a session's run directory is on disk, or that the check itself failed. */
export type DirectoryCheck = "present" | "missing" | "unknown";

/**
 * Decide what to show when Import opens and the server reports a session.
 *
 * Pure so the table can be read and tested on its own: the caller does the
 * network and filesystem work and hands the answers in.
 *
 * A session with no recorded device is treated as this install's.
 * `device_id` is optional on `POST /v1/imports`, so the server's `import`
 * command, or a program using an API token, opens a session without one, and
 * locking someone out of their own staged work over a missing field would
 * be worse than the rare case of two installs sharing a server.
 */
export function resumeDecisionFor(args: {
  session: ActiveImportRun | null;
  deviceId: string;
  directory: DirectoryCheck;
  fingerprint: FingerprintCheck;
}): ResumeDecision {
  const { session, deviceId, directory, fingerprint } = args;
  if (!session) return { kind: "none", session: null };
  if (session.device_id && session.device_id !== deviceId) {
    return { kind: "other_device", session };
  }
  if (!session.staging_dir || directory === "missing") {
    return { kind: "directory_missing", session };
  }
  if (directory === "unknown") {
    return { kind: "directory_unknown", session };
  }
  if (session.stage === "upload") {
    return { kind: "resume_upload", session };
  }
  if (session.stage === "staging_review" || session.stage === "media_review") {
    return { kind: "resume_review", session };
  }
  if (session.stage === "media") {
    return { kind: "resume_media", session };
  }
  // Only the copy cares whether the backup still matches: every later stage
  // works from the staged directory, not from the source.
  if (session.stage === "write") {
    if (fingerprint === "mismatch" || fingerprint === "source_missing") {
      return { kind: "source_changed", session };
    }
    return { kind: "resume_write", session };
  }
  return { kind: "restart", session };
}

/**
 * Whether acting on this decision runs extract over the backup again.
 *
 * A resumed Staging and both restarts do. A resume into a Review, Media, or
 * Upload works from the staged directory, and the rest only discard.
 */
export function resumeReadsBackup(kind: ResumeDecision["kind"]): boolean {
  return kind === "resume_write" || kind === "restart" || kind === "source_changed";
}

/** How a session's stored backup fingerprint compares to the backup now. */
export type FingerprintCheck = "match" | "mismatch" | "source_missing" | "unknown";

/**
 * Compare the fingerprint a session recorded against the source on disk.
 *
 * Directory sources carry the blind spot `buildSourceFingerprint` documents:
 * a stat of the directory entry does not move when a file inside it grows.
 * A change that goes unseen resumes and re-reads the backup, and unchanged
 * conversation boundaries keep the skip correct; this fires on every
 * difference the stat can actually see.
 */
export function checkSourceFingerprint(
  stored: SourceFingerprint | null,
  stat: PathStat | null,
): FingerprintCheck {
  if (!stored) return "unknown";
  if (!stat?.exists) return "source_missing";
  return stored.size_bytes === stat.sizeBytes && stored.modified_unix_ms === stat.modifiedUnixMs
    ? "match"
    : "mismatch";
}
