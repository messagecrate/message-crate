import type { ImportIssue } from "../../components/import/ImportSummaryPanel";
import { isIssueStage } from "../../components/import/importIssueStage";
import type { PushFinishedReport } from "../../lib/tauri";

/**
 * What an Import Run has recorded so far, across every part it ran in.
 *
 * A run runs in parts when it pauses, or stops at a Review, and resumes
 * later, perhaps after the app closed. The server takes the run's record
 * only with `/complete`, which a paused run never posts, so the window keeps
 * the record of the earlier parts in the staging folder
 * (`read_import_run_record`, `save_import_run_record`) and the completion
 * the last part posts covers the whole run: its Import Errors, timings,
 * bytes and counts.
 *
 * Durations and the Upload's counters add up across parts. The Staging
 * counts are the latest part's, since a resumed Staging reports on the
 * conversations it wrote itself.
 */
export type RunRecord = {
  issues: ImportIssue[];
  /**
   * The issues of the latest stopped part that `issues` leaves out, because
   * a resume reports them again or because they explain the stop
   * (`recordToCarry`). A resume never carries them; a Discard sends them,
   * since a discarded run is never resumed (`issuesToDiscard`).
   */
  lastStopIssues?: ImportIssue[];
  durationMs?: number;
  parseMs?: number;
  attachmentsMs?: number;
  prepareMs?: number;
  uploadMs?: number;
  bytesUploaded?: number;
  filesParsed?: number;
  messagesParsed?: number;
  /** Conversations an Upload sent whole. */
  filesSucceeded?: number;
  messagesAttempted?: number;
  messagesInserted?: number;
  messagesDeduped?: number;
  attachmentsUploaded?: number;
};

/** The record of a run with no earlier part. */
export const EMPTY_RUN_RECORD: RunRecord = { issues: [] };

const COUNT_FIELDS = [
  "durationMs",
  "parseMs",
  "attachmentsMs",
  "prepareMs",
  "uploadMs",
  "bytesUploaded",
  "filesParsed",
  "messagesParsed",
  "filesSucceeded",
  "messagesAttempted",
  "messagesInserted",
  "messagesDeduped",
  "attachmentsUploaded",
] as const;

function isIssue(value: unknown): value is ImportIssue {
  if (typeof value !== "object" || value === null) return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.kind === "string" &&
    isIssueStage(r.stage) &&
    typeof r.item === "string" &&
    typeof r.reason === "string"
  );
}

/**
 * Read a record back from the staging folder. The file is the app's own,
 * but it is read from disk, so each field is checked: an unreadable field
 * is left out, and anything that is not a record reads as no earlier part.
 */
export function parseRunRecord(raw: unknown): RunRecord {
  if (typeof raw !== "object" || raw === null) return EMPTY_RUN_RECORD;
  const r = raw as Record<string, unknown>;
  const record: RunRecord = {
    issues: Array.isArray(r.issues) ? r.issues.filter(isIssue) : [],
  };
  if (Array.isArray(r.lastStopIssues)) record.lastStopIssues = r.lastStopIssues.filter(isIssue);
  for (const field of COUNT_FIELDS) {
    const value = r[field];
    if (typeof value === "number" && Number.isFinite(value)) record[field] = value;
  }
  return record;
}

/** The part of the run on screen now, as the window measured it. */
export type RunPart = {
  issues: readonly ImportIssue[];
  durationMs: number;
  parseMs: number | null;
  attachmentsMs: number | null;
  prepareMs: number | null;
  uploadMs: number | null;
  filesParsed?: number;
  messagesParsed?: number;
  /** This part's push report, when it ran an Upload that reported. */
  report: PushFinishedReport | null;
};

/**
 * Whole milliseconds: the server stores durations as integers and refuses a
 * fraction, and `performance.now()` differences carry one.
 */
function sum(a: number | undefined, b: number | null | undefined): number | undefined {
  if (a == null && b == null) return undefined;
  return Math.round((a ?? 0) + (b ?? 0));
}

/** The whole run so far: the earlier parts' record with this part added. */
export function wholeRun(carried: RunRecord, part: RunPart): RunRecord {
  const report = part.report;
  return {
    issues: [...carried.issues, ...part.issues],
    durationMs: sum(carried.durationMs, part.durationMs),
    parseMs: sum(carried.parseMs, part.parseMs),
    attachmentsMs: sum(carried.attachmentsMs, part.attachmentsMs),
    prepareMs: sum(carried.prepareMs, part.prepareMs),
    uploadMs: sum(carried.uploadMs, part.uploadMs),
    bytesUploaded: sum(carried.bytesUploaded, report?.assets_bytes),
    filesParsed: part.filesParsed ?? carried.filesParsed,
    messagesParsed: part.messagesParsed ?? carried.messagesParsed,
    filesSucceeded: sum(carried.filesSucceeded, report?.conversations_ok),
    messagesAttempted: sum(carried.messagesAttempted, report?.messages_attempted),
    messagesInserted: sum(carried.messagesInserted, report?.messages_inserted),
    messagesDeduped: sum(carried.messagesDeduped, report?.messages_deduped),
    attachmentsUploaded: sum(carried.attachmentsUploaded, report?.assets_uploaded),
  };
}

/** The `item` of the error a stage records about the run as a whole. */
export const RUN_ERROR_ITEM = "Import";

/**
 * The record a stopped part leaves for the next one.
 *
 * Two kinds of issue are left out of its `issues`, and kept in
 * `lastStopIssues` for a Discard instead. An Upload's row for a whole
 * conversation (failed, or left unsent by a stop) is left out because the
 * resumed Upload sends that conversation again and reports it afresh. The
 * run-level error a stage records when it stops (item `Import`) is left out
 * because it explains the stop, not the run. An Upload's attachment skips
 * are kept, because the resumed Upload does not read those conversations
 * again.
 */
export function recordToCarry(carried: RunRecord, part: RunPart): RunRecord {
  const sentAgain = new Set(
    (part.report?.results ?? [])
      .filter((result) => result.status === "failed" || result.status === "cancelled")
      .map((result) => result.file),
  );
  const leftOut = (issue: ImportIssue) =>
    (issue.kind === "error" && issue.item === RUN_ERROR_ITEM) ||
    (issue.stage === "upload" && sentAgain.has(issue.item));
  const issues = part.issues.filter((issue) => !leftOut(issue));
  return {
    ...wholeRun(carried, { ...part, issues }),
    lastStopIssues: part.issues.filter(leftOut),
  };
}

/**
 * The Import Errors a Discard sends with the cancelled run: the whole
 * record's, with the latest stop's that a resume would have reported again.
 */
export function issuesToDiscard(record: RunRecord): ImportIssue[] {
  return [...record.issues, ...(record.lastStopIssues ?? [])];
}

/**
 * Conversations the run found already imported. A resumed Upload's report
 * counts the conversations an earlier part sent as skipped, since the push
 * journal lists them, so those are taken back out.
 */
export function filesSkippedOverRun(carried: RunRecord, report: PushFinishedReport): number {
  return Math.max(0, report.conversations_skipped - (carried.filesSucceeded ?? 0));
}
