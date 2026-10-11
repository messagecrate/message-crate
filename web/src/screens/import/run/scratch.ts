import type { ImportIssue, ImportNote } from "../../../components/import/ImportSummaryPanel";
import type { ImportIssueStage } from "../../../components/import/importIssueStage";
import type { ImportJobFormValues } from "../../../lib/importSources/types";
import { createRunCancel, type RunCancel } from "../../../lib/runCancel";
import { invokeReadImportRunRecord, type UploadFinishedReport } from "../../../lib/tauri";
import type { AttachmentMediaMode, ConversationStatus, StagedStatus } from "../../../lib/types";
import {
  type AttachmentProgressCounts,
  EMPTY_TIMING,
  type StageTiming,
} from "../importProgressState";
import { initialImportRunState, importRunStore as store } from "../importRunStore";
import { EMPTY_RUN_RECORD, parseRunRecord, type RunPart, type RunRecord } from "../runRecord";
import type { StagingProgressStep } from "./desktopJob";
import { initialSteps } from "./screen";

/** Parse/attachments/prepare durations, fixed once extract finishes and read again at finish time. */
type ExtractDurations = {
  parseMs: number | null;
  attachmentsMs: number | null;
  prepareMs: number | null;
};

const EMPTY_DURATIONS: ExtractDurations = { parseMs: null, attachmentsMs: null, prepareMs: null };

/**
 * What one run remembers between its stages and never shows: timings,
 * issues, counts, the submitted form. One run at a time, so one of these;
 * it lives beside the store so the run survives the screen unmounting.
 */
export type RunScratch = {
  /** The submitted form, parked while the identity stop is showing. */
  pendingIdentityForm: ImportJobFormValues | null;
  activeStage: ImportIssueStage;
  issues: ImportIssue[];
  /** The notes this part's stages sent, apart from its Import Errors. */
  notes: ImportNote[];
  /**
   * What this part's Upload said of each conversation file it finished so
   * far (`desktop-job:file-done`): `ok`, `skipped` or `failed`, by file.
   */
  conversations: Map<string, ConversationStatus>;
  /**
   * What this part's Staging said of each conversation file it finished so
   * far (`desktop-job:file-written`): `written` or `skipped`, by file.
   */
  staged: Map<string, StagedStatus>;
  counts: { filesParsed?: number; messagesParsed?: number };
  timing: StageTiming;
  durations: ExtractDurations;
  importStartedAt: number;
  form: ImportJobFormValues | null;
  attachmentMode: AttachmentMediaMode;
  lastAttachmentProgress: AttachmentProgressCounts | null;
  /**
   * The Staging row's latest line for each stage that reports on it. Reading
   * messages, copying attachments, and writing conversation files run at the
   * same time, so one shared line would flip between them on every event.
   */
  stagingLines: Partial<Record<StagingProgressStep, string>>;
  /** Guards approve and cancel against a double click doing the work twice. */
  reviewAction: boolean;
  /**
   * The Cancel of the stages running now. A new one starts with each run and
   * each approve, so a Cancel never carries into a stage the person started
   * after it.
   */
  runCancel: RunCancel;
  /**
   * Guards startImport the same way: the identity probe awaits two network
   * calls before runImport ever sets `running`, so a double-click on Import
   * while that probe is in flight would otherwise start two runs.
   */
  startImport: boolean;
  /**
   * What the run's earlier parts recorded, read from the run directory when
   * the run resumes (`runRecord.ts`). Empty for a run that started here.
   */
  carried: RunRecord;
};

function freshScratch(): RunScratch {
  return {
    pendingIdentityForm: null,
    activeStage: "staging",
    issues: [],
    notes: [],
    conversations: new Map(),
    staged: new Map(),
    counts: {},
    timing: { ...EMPTY_TIMING },
    durations: { ...EMPTY_DURATIONS },
    importStartedAt: 0,
    form: null,
    attachmentMode: "copy",
    lastAttachmentProgress: null,
    stagingLines: {},
    reviewAction: false,
    runCancel: createRunCancel(),
    startImport: false,
    carried: EMPTY_RUN_RECORD,
  };
}

let scratch: RunScratch = freshScratch();

/**
 * The scratch of the run in the store now. `resetImportRun` replaces it, so
 * a caller that must keep writing to the run it started holds on to what
 * this returned rather than calling it again.
 */
export function runScratch(): RunScratch {
  return scratch;
}

/** Put the store and the scratch back to a fresh form, started by no account. */
export function resetImportRun(): void {
  scratch = freshScratch();
  store.reset(initialImportRunState(initialSteps()));
}

resetImportRun();

/** Start a run's bookkeeping from nothing. */
export function beginRun(form: ImportJobFormValues, firstStage: ImportIssueStage): void {
  scratch.importStartedAt = performance.now();
  scratch.activeStage = firstStage;
  scratch.issues = [];
  scratch.notes = [];
  scratch.conversations = new Map();
  scratch.staged = new Map();
  scratch.counts = {};
  scratch.timing = { ...EMPTY_TIMING };
  scratch.durations = { ...EMPTY_DURATIONS };
  scratch.lastAttachmentProgress = null;
  scratch.stagingLines = {};
  scratch.attachmentMode = form.attachmentMedia;
  scratch.form = form;
  scratch.runCancel = createRunCancel();
  scratch.carried = EMPTY_RUN_RECORD;
}

/** This part of the run as it stands now. */
export function currentPart(
  report: UploadFinishedReport | null,
  uploadMs: number | null,
  run: RunScratch = scratch,
): RunPart {
  return {
    issues: run.issues,
    notes: run.notes,
    durationMs: performance.now() - run.importStartedAt,
    ...run.durations,
    uploadMs,
    filesParsed: run.counts.filesParsed,
    messagesParsed: run.counts.messagesParsed,
    conversations: run.conversations,
    staged: run.staged,
    report,
  };
}

/**
 * Pick up what the run's earlier parts recorded, from its run directory. A
 * record that cannot be read leaves the run with only what this part
 * records: the resume itself goes ahead.
 */
export async function loadCarriedRecord(runDir: string): Promise<void> {
  try {
    scratch.carried = parseRunRecord(await invokeReadImportRunRecord({ run_dir: runDir }));
  } catch {
    scratch.carried = EMPTY_RUN_RECORD;
  }
}
