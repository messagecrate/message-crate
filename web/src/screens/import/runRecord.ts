import type { ImportIssue } from "../../components/import/ImportSummaryPanel";
import { isIssueStage } from "../../components/import/importIssueStage";
import type { PushFinishedReport } from "../../lib/tauri";
import type { ConversationStatus } from "../../lib/types";

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
   * The issues `issues` leaves out because a resume may report them again
   * (an Upload's rows about a conversation not yet on the server) or because
   * they explain the latest stop (`recordToCarry`). A resume carries them
   * only once their conversation is on the server. A Discard sends them,
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

/** One issue read back from disk, or `undefined` when it is not one. */
function readIssue(value: unknown): ImportIssue | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const r = value as Record<string, unknown>;
  if (
    typeof r.kind !== "string" ||
    !isIssueStage(r.stage) ||
    typeof r.item !== "string" ||
    typeof r.reason !== "string"
  ) {
    return undefined;
  }
  const issue: ImportIssue = { kind: r.kind, stage: r.stage, item: r.item, reason: r.reason };
  if (typeof r.conversation === "string") issue.conversation = r.conversation;
  return issue;
}

function readIssues(value: unknown): ImportIssue[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((entry) => readIssue(entry) ?? []);
}

/**
 * Read a record back from the staging folder. The file is the app's own,
 * but it is read from disk, so each field is checked: an unreadable field
 * is left out, and anything that is not a record reads as no earlier part.
 */
export function parseRunRecord(raw: unknown): RunRecord {
  if (typeof raw !== "object" || raw === null) return EMPTY_RUN_RECORD;
  const r = raw as Record<string, unknown>;
  const record: RunRecord = { issues: readIssues(r.issues) };
  if (Array.isArray(r.lastStopIssues)) record.lastStopIssues = readIssues(r.lastStopIssues);
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
  /**
   * What this part's Upload said of each conversation file it finished so
   * far (`extract:file-done`), by file: `ok`, `skipped` or `failed`.
   */
  conversations: ReadonlyMap<string, ConversationStatus>;
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

/** Two issues are one row when every field matches. */
function issueKey(issue: ImportIssue): string {
  return JSON.stringify([issue.kind, issue.stage, issue.item, issue.reason, issue.conversation]);
}

/**
 * `earlier`, then the rows of `later` that are not already in it. A stage
 * that resumes reads again what it had not finished, and reports the same
 * rows again: each is kept once. Counted, so a row `earlier` holds twice
 * absorbs two of `later`'s and no more.
 */
export function mergeIssues(
  earlier: readonly ImportIssue[],
  later: readonly ImportIssue[],
): ImportIssue[] {
  const unmatched = new Map<string, number>();
  for (const issue of earlier) {
    const key = issueKey(issue);
    unmatched.set(key, (unmatched.get(key) ?? 0) + 1);
  }
  const merged = [...earlier];
  for (const issue of later) {
    const key = issueKey(issue);
    const count = unmatched.get(key) ?? 0;
    if (count > 0) unmatched.set(key, count - 1);
    else merged.push(issue);
  }
  return merged;
}

/** The `item` of the error a stage records about the run as a whole. */
export const RUN_ERROR_ITEM = "Import";

function isRunError(issue: ImportIssue): boolean {
  return issue.kind === "error" && issue.item === RUN_ERROR_ITEM;
}

/**
 * What this part's Upload said of each conversation: the files it finished
 * as they finished, and every file its report lists, which adds the ones a
 * stop left unsent (`cancelled`).
 */
function conversationStatuses(part: RunPart): Map<string, ConversationStatus> {
  const statuses = new Map(part.conversations);
  for (const result of part.report?.results ?? []) statuses.set(result.file, result.status);
  return statuses;
}

/** On the server: sent by this part, or by an earlier one (the journal skipped it). */
function isOnServer(status: ConversationStatus | undefined): boolean {
  return status === "ok" || status === "skipped";
}

/**
 * Sort an earlier stop's rows by what this part's Upload said of their
 * conversation. A row about a whole conversation this Upload reported on is
 * stale, and goes: this Upload reports that conversation itself. A
 * conversation this Upload sent (`ok`) was read again, so its other earlier
 * rows were reported afresh, and go. Those of a conversation an earlier
 * part sent (`skipped`) are true and final, and are promoted. The rest
 * still wait: a `failed` conversation may have failed before it was read,
 * and rows this part reported again are merged with them (`mergeIssues`).
 */
function sortEarlierStop(
  carried: RunRecord,
  statuses: ReadonlyMap<string, ConversationStatus>,
): { promoted: ImportIssue[]; waiting: ImportIssue[] } {
  const promoted: ImportIssue[] = [];
  const waiting: ImportIssue[] = [];
  for (const issue of carried.lastStopIssues ?? []) {
    if (isRunError(issue)) continue;
    const status = issue.conversation == null ? undefined : statuses.get(issue.conversation);
    const wholeConversation = issue.item === issue.conversation;
    if (status === "ok" || (wholeConversation && status != null)) continue;
    if (status === "skipped") promoted.push(issue);
    else waiting.push(issue);
  }
  return { promoted, waiting };
}

/** The whole run so far: the earlier parts' record with this part added. */
export function wholeRun(carried: RunRecord, part: RunPart): RunRecord {
  const report = part.report;
  const { promoted } = sortEarlierStop(carried, conversationStatuses(part));
  return {
    issues: mergeIssues(carried.issues, mergeIssues(promoted, part.issues)),
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

/**
 * The record for the next part of the run, as the run stands now: written
 * when a part stops, and while a stage runs, as each issue arrives, so an
 * app that closes mid-stage leaves it in the Staging Directory.
 *
 * Two kinds of issue are left out of its `issues`, and kept in
 * `lastStopIssues` for a Discard instead. An Upload's row about a
 * conversation not yet on the server is left out, because a resumed Upload
 * reads that conversation again and reports it afresh; it moves to `issues`
 * once its conversation is sent. The run-level error a stage records when
 * it stops (item `Import`) is left out because it explains the stop, not
 * the run. An earlier stop's rows are sorted as `sortEarlierStop` says.
 */
export function recordToCarry(carried: RunRecord, part: RunPart): RunRecord {
  const statuses = conversationStatuses(part);
  const waits = (issue: ImportIssue) =>
    issue.conversation != null && !isOnServer(statuses.get(issue.conversation));
  const rows = part.issues.filter((issue) => !isRunError(issue));
  const { waiting } = sortEarlierStop(carried, statuses);
  return {
    ...wholeRun(carried, { ...part, issues: rows.filter((issue) => !waits(issue)) }),
    lastStopIssues: [
      ...mergeIssues(waiting, rows.filter(waits)),
      ...part.issues.filter(isRunError),
    ],
  };
}

/**
 * `rows` without those `resolved` says no longer hold: the rows with its
 * stage and item.
 */
export function withoutResolved(
  rows: readonly ImportIssue[],
  resolved: Pick<ImportIssue, "stage" | "item">,
): ImportIssue[] {
  return rows.filter((row) => row.stage !== resolved.stage || row.item !== resolved.item);
}

/** `record` without the rows `resolved` says no longer hold (`withoutResolved`). */
export function resolveInRecord(
  record: RunRecord,
  resolved: Pick<ImportIssue, "stage" | "item">,
): RunRecord {
  const next: RunRecord = { ...record, issues: withoutResolved(record.issues, resolved) };
  if (record.lastStopIssues != null) {
    next.lastStopIssues = withoutResolved(record.lastStopIssues, resolved);
  }
  return next;
}

/**
 * The Import Errors a Discard sends with the cancelled run: the whole
 * record's, with the latest stop's that a resume would have reported again.
 */
export function issuesToDiscard(record: RunRecord): ImportIssue[] {
  return mergeIssues(record.issues, record.lastStopIssues ?? []);
}

/**
 * The issues as the server takes them, without the conversation an Upload
 * row names for the record's own use.
 */
export function issueRequests(issues: readonly ImportIssue[]): ImportIssue[] {
  return issues.map(({ kind, stage, item, reason }) => ({ kind, stage, item, reason }));
}

/**
 * Conversations the run found already imported. A resumed Upload's report
 * counts the conversations an earlier part sent as skipped, since the push
 * journal lists them, so those are taken back out.
 */
export function filesSkippedOverRun(carried: RunRecord, report: PushFinishedReport): number {
  return Math.max(0, report.conversations_skipped - (carried.filesSucceeded ?? 0));
}
