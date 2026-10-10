import type { ImportIssue, ImportNote } from "../../components/import/ImportSummaryPanel";
import { isIssueStage } from "../../components/import/importIssueStage";
import type { UploadFinishedReport } from "../../lib/tauri";
import type { ConversationStatus, StagedStatus } from "../../lib/types";

/**
 * What an Import Run has recorded so far, across every part it ran in.
 *
 * A run runs in parts when it pauses, or stops at a Review, and resumes
 * later, perhaps after the app closed. The server takes the run's record
 * only with `/complete`, which a paused run never posts, so the window keeps
 * the record of the earlier parts in the run directory
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
   * (an Upload's rows about a conversation not yet on the server, and a
   * Staging row about a conversation not yet written) or because they
   * explain the latest stop (`recordToCarry`). A resume carries them only
   * once their conversation is on the server, or written. A Discard sends
   * them, since a discarded run is never resumed (`issuesToDiscard`).
   */
  lastStopIssues?: ImportIssue[];
  /**
   * The run's notes, apart from its Import Errors; absent when it noted
   * nothing. A note is about an item a stage read, never about whether a
   * conversation reached the server, so every part's notes are kept, except
   * that a resumed Staging that has read the whole backup again replaces the
   * earlier parts' Staging notes with its own (`combine`).
   */
  notes?: ImportNote[];
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

/** Every kind a row can have, so a stored issue's `kind` is checked against it. */
const ISSUE_KINDS: Record<ImportIssue["kind"], true> = {
  skip: true,
  error: true,
  note: true,
  resolved: true,
};

function isIssueKind(value: unknown): value is ImportIssue["kind"] {
  return typeof value === "string" && Object.hasOwn(ISSUE_KINDS, value);
}

/** One issue read back from disk, or `undefined` when it is not one. */
function readIssue(value: unknown): ImportIssue | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const r = value as Record<string, unknown>;
  if (
    !isIssueKind(r.kind) ||
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

/** One note read back from disk, or `undefined` when it is not one. */
function readNote(value: unknown): ImportNote | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const r = value as Record<string, unknown>;
  if (!isIssueStage(r.stage) || typeof r.item !== "string" || typeof r.text !== "string") {
    return undefined;
  }
  return { stage: r.stage, item: r.item, text: r.text };
}

/**
 * Read a record back from the run directory. The file is the app's own,
 * but it is read from disk, so each field is checked: an unreadable field
 * is left out, and anything that is not a record reads as no earlier part.
 */
export function parseRunRecord(raw: unknown): RunRecord {
  if (typeof raw !== "object" || raw === null) return EMPTY_RUN_RECORD;
  const r = raw as Record<string, unknown>;
  const record: RunRecord = { issues: readIssues(r.issues) };
  if (Array.isArray(r.lastStopIssues)) record.lastStopIssues = readIssues(r.lastStopIssues);
  if (Array.isArray(r.notes)) record.notes = r.notes.flatMap((entry) => readNote(entry) ?? []);
  for (const field of COUNT_FIELDS) {
    const value = r[field];
    if (typeof value === "number" && Number.isFinite(value)) record[field] = value;
  }
  return record;
}

/** The part of the run on screen now, as the window measured it. */
export type RunPart = {
  issues: readonly ImportIssue[];
  /** The notes this part's stages sent; absent when they sent none. */
  notes?: readonly ImportNote[];
  durationMs: number;
  parseMs: number | null;
  attachmentsMs: number | null;
  prepareMs: number | null;
  uploadMs: number | null;
  filesParsed?: number;
  messagesParsed?: number;
  /**
   * What this part's Upload said of each conversation file it finished so
   * far (`desktop-job:file-done`), by file: `ok`, `skipped` or `failed`.
   */
  conversations: ReadonlyMap<string, ConversationStatus>;
  /**
   * What this part's Staging said of each conversation file it finished so
   * far (`desktop-job:file-written`), by file: `written` or `skipped`.
   */
  staged: ReadonlyMap<string, StagedStatus>;
  /** This part's Upload report, when it ran an Upload that reported. */
  report: UploadFinishedReport | null;
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

/** Two notes are one row when every field matches. */
function noteKey(note: ImportNote): string {
  return JSON.stringify([note.stage, note.item, note.text]);
}

/**
 * `earlier`, then the rows of `later` that are not already in it, as `keyOf`
 * tells rows apart. Counted, so a row `earlier` holds twice absorbs two of
 * `later`'s and no more.
 */
function mergeOnce<T>(earlier: readonly T[], later: readonly T[], keyOf: (row: T) => string): T[] {
  const unmatched = new Map<string, number>();
  for (const row of earlier) {
    const key = keyOf(row);
    unmatched.set(key, (unmatched.get(key) ?? 0) + 1);
  }
  const merged = [...earlier];
  for (const row of later) {
    const key = keyOf(row);
    const count = unmatched.get(key) ?? 0;
    if (count > 0) unmatched.set(key, count - 1);
    else merged.push(row);
  }
  return merged;
}

/**
 * `earlier`, then the rows of `later` that are not already in it. A stage
 * that resumes reads again what it had not finished, and reports the same
 * rows again: each is kept once.
 */
export function mergeIssues(
  earlier: readonly ImportIssue[],
  later: readonly ImportIssue[],
): ImportIssue[] {
  return mergeOnce(earlier, later, issueKey);
}

/** `earlier`, then the notes of `later` it does not already hold (`mergeIssues`). */
function mergeNotes(
  earlier: readonly ImportNote[] | undefined,
  later: readonly ImportNote[] | undefined,
): ImportNote[] | undefined {
  const merged = mergeOnce(earlier ?? [], later ?? [], noteKey);
  return merged.length > 0 ? merged : undefined;
}

/** The `item` of the error a stage records about the run as a whole. */
export const RUN_ERROR_ITEM = "Import";

function isRunError(issue: ImportIssue): boolean {
  return issue.kind === "error" && issue.item === RUN_ERROR_ITEM;
}

/**
 * What this part's Upload said of each conversation: the files it finished
 * as they finished, and every file its report lists, which adds the ones a
 * pause left unsent (`cancelled`).
 */
function conversationStatuses(part: RunPart): Map<string, ConversationStatus> {
  const statuses = new Map(part.conversations);
  for (const result of part.report?.results ?? []) {
    if (isConversationStatus(result.status)) statuses.set(result.file, result.status);
  }
  return statuses;
}

const CONVERSATION_STATUSES: readonly string[] = ["ok", "skipped", "failed", "cancelled"];

/** A report's status for a conversation, as one the record knows. */
function isConversationStatus(status: string): status is ConversationStatus {
  return CONVERSATION_STATUSES.includes(status);
}

/** On the server: sent by this part, or by an earlier one (the journal skipped it). */
function isOnServer(status: ConversationStatus | undefined): boolean {
  return status === "ok" || status === "skipped";
}

/** What this part said of the conversations its rows are about. */
type PartConversations = {
  /** By its Upload (`conversationStatuses`). */
  uploaded: ReadonlyMap<string, ConversationStatus>;
  /** By its Staging (`RunPart.staged`). */
  staged: ReadonlyMap<string, StagedStatus>;
};

function partConversations(part: RunPart): PartConversations {
  return { uploaded: conversationStatuses(part), staged: part.staged };
}

/**
 * A Staging row about one conversation: one the exporter recorded while the
 * write queue wrote that conversation, such as an attachment the iPhone
 * exporter could not decrypt. A resumed Staging skips a conversation already
 * written without reading it again, so it reports such a row again only
 * while the conversation is not yet written.
 */
export function isStagingRowOfConversation(
  issue: ImportIssue,
): issue is ImportIssue & { conversation: string } {
  return issue.stage === "staging" && issue.conversation != null;
}

/**
 * A Staging row recorded while the exporter read the backup, such as a file
 * it could not read: one that names no conversation. Every Staging reads the
 * whole backup before it writes anything, so a resumed Staging reports every
 * such row again that still holds (#1947).
 */
function isBackupReadRow(issue: ImportIssue): boolean {
  return issue.stage === "staging" && issue.conversation == null;
}

/**
 * Whether this part's Staging has read the whole backup: its write queue
 * has said what it did with a conversation, which it does only once the read
 * is done, or the Staging has finished. A part resumed past Staging reads
 * nothing again, and a Staging stopped while it reads has not read it all.
 */
function hasReadWholeBackup(part: RunPart): boolean {
  return part.staged.size > 0 || part.filesParsed != null;
}

/**
 * Whether a row this part reported may be reported again by a resume, and
 * so waits apart: a Staging row about a conversation this part's Staging has
 * not yet written, or an Upload row about a conversation not yet on the
 * server.
 */
function waits(issue: ImportIssue, part: PartConversations): boolean {
  if (issue.conversation == null) return false;
  if (isStagingRowOfConversation(issue)) return !part.staged.has(issue.conversation);
  return !isOnServer(part.uploaded.get(issue.conversation));
}

/**
 * Sort an earlier pause's rows by what this part said of their
 * conversation.
 *
 * A Staging row goes once this part's Staging wrote its conversation
 * (`written`): that Staging read the conversation again and reported its
 * rows afresh. A Staging row about a conversation this part's Staging
 * skipped (`skipped`) is true and final, and is promoted, since no later
 * Staging reads that conversation again. Nothing the Upload says changes a
 * Staging row.
 *
 * An Upload row about a whole conversation this Upload reported on is
 * stale, and goes: this Upload reports that conversation itself. A
 * conversation this Upload sent (`ok`) was read again, so its other earlier
 * rows were reported afresh, and go. Those of a conversation an earlier
 * part sent (`skipped`) are true and final, and are promoted.
 *
 * The rest still wait: a `failed` conversation may have failed before it
 * was read, and rows this part reported again are merged with them
 * (`mergeIssues`).
 */
function sortEarlierStop(
  carried: RunRecord,
  part: PartConversations,
): { promoted: ImportIssue[]; waiting: ImportIssue[] } {
  const promoted: ImportIssue[] = [];
  const waiting: ImportIssue[] = [];
  for (const issue of carried.lastStopIssues ?? []) {
    if (isRunError(issue)) continue;
    if (isStagingRowOfConversation(issue)) {
      const staged = part.staged.get(issue.conversation);
      if (staged === "written") continue;
      if (staged === "skipped") promoted.push(issue);
      else waiting.push(issue);
      continue;
    }
    const status = issue.conversation == null ? undefined : part.uploaded.get(issue.conversation);
    const wholeConversation = issue.item === issue.conversation;
    if (status === "ok" || (wholeConversation && status != null)) continue;
    if (status === "skipped") promoted.push(issue);
    else waiting.push(issue);
  }
  return { promoted, waiting };
}

/**
 * The whole run so far, as a completion posts it: the earlier parts' record
 * with this part added. Its issues take in every row of an earlier stop
 * that still stands (`sortEarlierStop`), promoted or waiting, since a run
 * that completes is never resumed.
 */
export function wholeRun(carried: RunRecord, part: RunPart): RunRecord {
  const { promoted, waiting } = sortEarlierStop(carried, partConversations(part));
  return combine(carried, part, [...promoted, ...waiting]);
}

/**
 * The earlier parts' record with this part added, taking in `earlier`, the
 * rows of the earlier stop that join `issues`. Once this part's Staging has
 * read the whole backup, its own rows and notes from that read replace the
 * earlier parts' (`isBackupReadRow`), so such a row or note stays only while
 * it still holds (#1947). Every Staging note is sent while the exporter
 * reads, so the resumed Staging sends again each one that still holds.
 */
function combine(carried: RunRecord, part: RunPart, earlier: ImportIssue[]): RunRecord {
  const report = part.report;
  const readAgain = hasReadWholeBackup(part);
  const carriedNotes = readAgain
    ? carried.notes?.filter((note) => note.stage !== "staging")
    : carried.notes;
  const notes = mergeNotes(carriedNotes, part.notes);
  const carriedIssues = readAgain
    ? carried.issues.filter((issue) => !isBackupReadRow(issue))
    : carried.issues;
  return {
    issues: mergeIssues(carriedIssues, mergeIssues(earlier, part.issues)),
    ...(notes ? { notes } : {}),
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
 * app that closes mid-stage leaves it in the run directory.
 *
 * Three kinds of issue are left out of its `issues`, and kept in
 * `lastStopIssues` for a Discard instead. An Upload's row about a
 * conversation not yet on the server is left out, because a resumed Upload
 * reads that conversation again and reports it afresh; it moves to `issues`
 * once its conversation is sent. A Staging row about a conversation not yet
 * written is left out for the same reason, and moves to `issues` once the
 * write queue writes that conversation (#1688). The run-level error a stage
 * records when it stops (item `Import`) is left out because it explains the
 * stop, not the run. An earlier stop's rows are sorted as `sortEarlierStop`
 * says.
 */
export function recordToCarry(carried: RunRecord, part: RunPart): RunRecord {
  const conversations = partConversations(part);
  const waitsNow = (issue: ImportIssue) => waits(issue, conversations);
  const rows = part.issues.filter((issue) => !isRunError(issue));
  const { promoted, waiting } = sortEarlierStop(carried, conversations);
  return {
    ...combine(carried, { ...part, issues: rows.filter((issue) => !waitsNow(issue)) }, promoted),
    lastStopIssues: [
      ...mergeIssues(waiting, rows.filter(waitsNow)),
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

/** The notes a Discard sends with the cancelled run: every part's. */
export function notesToDiscard(record: RunRecord): ImportNote[] {
  return record.notes ?? [];
}

/**
 * The issues as the server takes them, without the conversation an Upload
 * or Staging row names for the record's own use.
 */
export function issueRequests(issues: readonly ImportIssue[]): ImportIssue[] {
  return issues.map(({ kind, stage, item, reason }) => ({ kind, stage, item, reason }));
}

/**
 * Conversations the run found already imported. A resumed Upload's report
 * counts the conversations an earlier part sent as skipped, since the Upload
 * journal lists them, so those are taken back out.
 */
export function filesSkippedOverRun(carried: RunRecord, report: UploadFinishedReport): number {
  return Math.max(0, report.conversations_skipped - (carried.filesSucceeded ?? 0));
}
