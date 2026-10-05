import {
  completionTextFor,
  type ImportIssue,
  type ImportNote,
  type ImportSummaryView,
} from "../../components/import/ImportSummaryPanel";
import type { ImportIssueStage } from "../../components/import/importIssueStage";
import { getAccountId, getBaseUrl } from "../../lib/api";
import { formatAttachmentProgress } from "../../lib/attachmentProgressCopy";
import { useAuth } from "../../lib/auth";
import { needsIdentityStop, parseSourceIdentities } from "../../lib/backupIdentity";
import { getDeviceId } from "../../lib/deviceId";
import { IMAZING_SOURCE_ID } from "../../lib/exportSources";
import { imessageExtractFields } from "../../lib/imessageExtractFields";
import { isImessageMethod } from "../../lib/imessageImport";
import type { ActiveImportRun } from "../../lib/importRun";
import {
  buildSourceFingerprint,
  discardImportRun,
  type ImportStage,
  setImportStage,
} from "../../lib/importRun";
import { importRunCreateBody, showsAttachmentOptions } from "../../lib/importSource";
import { CANCELLED_MESSAGE, createRunCancel, type RunCancel } from "../../lib/runCancel";
import { registerRunningUpload, uploadSessionRefused } from "../../lib/runningUpload";
import { sbrExtractFields } from "../../lib/sbrExtractFields";
import { completeImport, createImport, getServerState } from "../../lib/serverApi";
import {
  type AttachmentForecast,
  awaitTauriJob,
  invokeCreateStagingDir,
  invokeDeleteStaging,
  invokeExtract,
  invokeImessageBackupIdentities,
  invokePathStat,
  invokeUpload,
  invokeReadImportRunRecord,
  invokeSaveImportRunRecord,
  invokeSummarizeStaging,
  invokeTranscodeStaging,
  type OwnerIdentityCount,
  onExtractEvents,
  type UploadFinishedReport,
  probeFfmpegTools,
  type SizeVerdict,
  type StagingConfig,
  type StagingSummary,
  type TauriJobResult,
  type TranscodeFinishedReport,
} from "../../lib/tauri";
import { isTauri } from "../../lib/tauri-check";
import type {
  AttachmentMediaMode,
  ConversationStatus,
  ImportFileDoneEvent,
  ImportIssueEvent,
  ImportProgressEvent,
} from "../../lib/types";
import { useFetchAccountProfile } from "../../lib/useAccountProfile";
import { whatsappExtractFields } from "../../lib/whatsappExtractFields";
import { isWhatsappMethod } from "../../lib/whatsappImport";
import { formSnapshot, isAttachmentMediaMode, isStringArray } from "./formSnapshot";
import { mediaJobVerb } from "./reviewForecast";
import { importOutcome } from "./importOutcome";
import {
  type AttachmentProgressCounts,
  attachmentDoneDetail,
  EMPTY_TIMING,
  type ImportStep,
  isProgressStepComplete,
  issueFromEvent,
  MEDIA_LABEL,
  noteFromEvent,
  recordStageTime,
  STAGING_LABEL,
  type StageTiming,
  setupDetail,
  stageDurations,
  stageForStep,
  stepIndexFor,
  stepsFor,
  UPLOAD_LABEL,
} from "./importProgressState";
import {
  type ImportRunState,
  importRunStore,
  initialImportRunState,
  useImportRunState,
} from "./importRunStore";
import {
  EMPTY_RUN_RECORD,
  filesSkippedOverRun,
  issueRequests,
  issuesToDiscard,
  notesToDiscard,
  parseRunRecord,
  RUN_ERROR_ITEM,
  type RunPart,
  type RunRecord,
  recordToCarry,
  resolveInRecord,
  wholeRun,
  withoutResolved,
} from "./runRecord";

export type { ImportPhase, ImportStep } from "./importProgressState";

/** Parse/attachments/prepare durations, fixed once extract finishes and read again at finish time. */
type ExtractDurations = {
  parseMs: number | null;
  attachmentsMs: number | null;
  prepareMs: number | null;
};

const EMPTY_DURATIONS: ExtractDurations = { parseMs: null, attachmentsMs: null, prepareMs: null };

/** Present-tense verb for the media step, following the mode so compress mode never says "Converting". */
function mediaVerb(mode: AttachmentMediaMode): string {
  return mode === "compress" ? "Compressing" : "Converting";
}

/** Sentence shown on the media step's row once the pass finishes. */
function mediaDoneDetail(mode: AttachmentMediaMode): string {
  return mode === "compress" ? "Compression complete" : "Conversion complete";
}

/**
 * The form an Import Run is started with, its attachment mode as the form
 * showed it. iMazing and OpenExtract show no Attachments field, so the
 * field may still hold what was chosen for another source; their runs copy
 * attachments, and the run's stored form says so.
 */
function withShownAttachmentMode(form: ImportJobFormValues): ImportJobFormValues {
  return showsAttachmentOptions(form.source) ? form : { ...form, attachmentMedia: "copy" };
}

/**
 * The run's form with the attachment mode its run directory recorded.
 *
 * Staging records the run's media settings in the directory, and the summary
 * of the directory carries the mode. From the end of Staging on, the directory is
 * the one source of the mode, so whether the run has a Media stage is read
 * from there and never from the stored form. `summary` is absent before
 * Staging has finished, and when a resumed run's stored plan no longer
 * parses; the form's own mode stands then, because nothing else holds one.
 */
function withRecordedMode(
  form: ImportJobFormValues,
  summary: StagingSummary | null | undefined,
): ImportJobFormValues {
  if (!summary) return form;
  return { ...form, attachmentMedia: summary.mediaMode };
}

/**
 * `withRecordedMode`, made the run's own form: every later stage, the
 * progress rows and the review screens read the directory's mode from here.
 */
function adoptRecordedMode(
  form: ImportJobFormValues,
  summary: StagingSummary | null | undefined,
): ImportJobFormValues {
  const recorded = withRecordedMode(form, summary);
  scratch.form = recorded;
  scratch.attachmentMode = recorded.attachmentMedia;
  store.set({ form: recorded });
  return recorded;
}

/**
 * The attachment size limit Staging works to, in bytes. A run always has one
 * by the time Staging needs it: a new run reads the server's before Staging,
 * and a resumed Staging reads its own back from the stored form. Upload
 * reads the limit Staging recorded in the directory instead, so it is not
 * passed on.
 */
function assetLimitOf(form: Pick<ImportJobFormValues, "assetMaxBytes">): number {
  if (typeof form.assetMaxBytes !== "number") {
    throw new Error("This Import Run has no attachment size limit stored with it.");
  }
  return form.assetMaxBytes;
}

/** What a step is doing and what it counts, for every step but `media`
 * (which needs the mode — see `mediaVerb`), `setup` (which carries its own
 * label — see `setupDetail`) and `attachments` (which adds bytes — see
 * `formatAttachmentProgress`), keyed by step name so a step added to the
 * wire union without an entry here is a compile error rather than a silent
 * fallback.
 */
const STEP_LABEL: Record<
  Exclude<ImportProgressEvent["step"], "media" | "setup" | "attachments">,
  string
> = {
  parse: "Reading messages",
  prepare: "Preparing conversations",
  check: "Checking attachments",
  upload: "Uploading conversations",
};

/**
 * Label shown while a step is running. Falls back to a plain word for a
 * step string this build doesn't recognise — the event comes off the wire
 * unvalidated.
 */
function progressLabel(
  step: Exclude<ImportProgressEvent["step"], "setup" | "attachments">,
  mode: AttachmentMediaMode,
): string {
  if (step === "media") return `${mediaVerb(mode)} attachments`;
  return STEP_LABEL[step] ?? "Working";
}

/** Stage rows for this mode, with Staging optionally marked active. */
function initialSteps(
  status: ImportStep["status"] = "pending",
  attachmentMedia: AttachmentMediaMode = "copy",
): ImportStep[] {
  const steps = stepsFor(attachmentMedia);
  const first = steps[0];
  if (status === "active" && first) {
    steps[0] = { ...first, status, detail: "Reading backup…" };
  }
  return steps;
}

/**
 * Stage rows for a run resumed at a review or mid Media: Staging is
 * already done (nothing here re-extracts), Upload is always still pending
 * (nothing here has uploaded yet), and Media (when this mode has it) is done
 * only when `mediaDone` says the pass already finished in an earlier run. A
 * resume at `media` passes `mediaDone: false` and then calls
 * `runMediaPass`, which marks that same row active once it starts.
 */
function resumeSteps(attachmentMedia: AttachmentMediaMode, mediaDone: boolean): ImportStep[] {
  return stepsFor(attachmentMedia).map((step) => {
    if (step.label === STAGING_LABEL) return { ...step, status: "done", detail: "Already staged" };
    if (step.label === MEDIA_LABEL && mediaDone) {
      return { ...step, status: "done", detail: mediaDoneDetail(attachmentMedia) };
    }
    return step;
  });
}

export type ImportJobFormValues = {
  source: string;
  backupPath: string;
  backupPassword: string;
  attachmentMedia: AttachmentMediaMode;
  maxResolution: string;
  maxFps: string;
  minSizeMb: string;
  ownerPhones: string[];
  /** Owner email addresses; only SMS Backup+ reads them. */
  ownerEmails: string[];
  obfuscate: boolean;
  /** The IANA zone iMazing dates are read in: the account's, or the one picked
   * under Processing Options. Only the iMazing extract reads it, because its
   * dates carry no zone of their own. */
  timeZone: string;
  /** True for the Android SMS sources, whose extract carries owner phones. */
  isAndroidSms: boolean;
  attachmentRoot: string;
  appleContacts: string;
  whatsappKey: string;
  whatsappWa: string;
  whatsappMedia: string;
  whatsappDb: string;
  whatsappBusiness: boolean;
  /** The holder's WhatsApp number: required on Android, a fallback on iPhone. */
  whatsappOwnerPhone: string;
  /**
   * The server's attachment size limit, in bytes, as this Import Run works
   * to it. Not a field the person fills in: a new run reads it from
   * `GET /v1/server` before Staging, and it is stored with the run in the
   * form snapshot, so a resume uses the number the run was staged and
   * reviewed against even when the owner has changed the limit since.
   */
  assetMaxBytes?: number;
};

/** A session whose copy was interrupted, and the directory it was writing into. */
export type ResumeWrite = {
  sessionId: number;
  stagingDir: string;
  /** The list recorded on the session at creation (`session.source_identities`,
   * parsed by the caller). A resumed write lands back on Gate 1 without
   * re-probing the backup, so this is the only way that gate's identity
   * section gets a list to show. */
  identities?: string[] | null;
};

/** Pick up a session whose run directory is already complete. */
export type ResumeUpload = {
  sessionId: number;
  stagingDir: string;
  /** The plan approved at the last gate this session passed, parsed from
   * its stored `summary` (`parseStoredStagingSummary`). Undefined when the
   * session recorded nothing usable — `runUpload` and `importOutcome`
   * tolerate that absence, they just can't diff a resumed push's expected
   * omissions against it, which demotes an honest `completed` outcome to
   * `completed_with_issues` for exactly the interrupted-and-resumed case. */
  approved?: StagingSummary;
};

const SIZE_VERDICTS: readonly SizeVerdict[] = [
  "fits_as_is",
  "likely_fits",
  "may_grow",
  "probably_too_big",
  "cannot_process",
];

function isAttachmentForecast(value: unknown): value is AttachmentForecast {
  if (typeof value !== "object" || value === null) return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.path === "string" &&
    typeof r.name === "string" &&
    typeof r.sizeBytes === "number" &&
    typeof r.estimateBytes === "number" &&
    typeof r.verdict === "string" &&
    SIZE_VERDICTS.includes(r.verdict as SizeVerdict)
  );
}

function isOwnerIdentityCount(value: unknown): value is OwnerIdentityCount {
  if (typeof value !== "object" || value === null) return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.identity === "string" && typeof r.sent === "number" && typeof r.received === "number"
  );
}

/**
 * Parse a session's stored `summary` back into a `StagingSummary`
 * — the plan approved at the last gate the session passed.
 *
 * Read only as the *approved baseline* on resume, never shown directly:
 * the summary actually on screen is always recomputed
 * fresh from the directory. Like `restoreFormFromSnapshot`, this value came
 * from the database rather than from this session's own state, so its
 * shape is checked field by field rather than trusted; returns `undefined`
 * — not a throw — for anything that doesn't match. A resume with no usable
 * baseline still proceeds: `importOutcome` tolerates an absent one, it just
 * can't diff against one.
 */
export function parseStoredStagingSummary(raw: unknown): StagingSummary | undefined {
  if (typeof raw !== "object" || raw === null) return undefined;
  const r = raw as Record<string, unknown>;
  if (typeof r.conversations !== "number") return undefined;
  if (typeof r.messages !== "number") return undefined;
  if (!isStringArray(r.contactIdentifiers)) return undefined;
  if (!Array.isArray(r.ownerIdentities) || !r.ownerIdentities.every(isOwnerIdentityCount)) {
    return undefined;
  }
  if (typeof r.attachments !== "number") return undefined;
  if (typeof r.attachmentBytes !== "number") return undefined;
  if (!Array.isArray(r.forecasts) || !r.forecasts.every(isAttachmentForecast)) return undefined;
  if (typeof r.assetMaxBytes !== "number") return undefined;
  if (!isAttachmentMediaMode(r.mediaMode)) return undefined;

  return {
    conversations: r.conversations,
    messages: r.messages,
    contactIdentifiers: r.contactIdentifiers,
    ownerIdentities: r.ownerIdentities,
    attachments: r.attachments,
    attachmentBytes: r.attachmentBytes,
    forecasts: r.forecasts,
    assetMaxBytes: r.assetMaxBytes,
    mediaMode: r.mediaMode,
  };
}

/**
 * What one run remembers between its stages and never shows: timings,
 * issues, counts, the submitted form. One run at a time, so one of these;
 * it lives beside the store so the run survives the screen unmounting.
 */
type RunScratch = {
  /** The submitted form, parked while the identity stop is showing. */
  pendingIdentityForm: ImportJobFormValues | null;
  activeStage: ImportIssueStage;
  issues: ImportIssue[];
  /** The notes this part's stages sent, apart from its Import Errors. */
  notes: ImportNote[];
  /**
   * What this part's Upload said of each conversation file it finished so
   * far (`extract:file-done`): `ok`, `skipped` or `failed`, by file.
   */
  conversations: Map<string, ConversationStatus>;
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

const store = importRunStore;

/** Put the store and the scratch back to a fresh form, started by no account. */
export function resetImportRun(): void {
  scratch = freshScratch();
  store.reset(initialImportRunState(initialSteps()));
}

resetImportRun();

/**
 * What the Import screen's actions are doing now, whichever account started
 * them: a stage, a probe, a Review being cancelled. Settles once they stop.
 * `takeRunFor` waits on it before another account's run replaces them.
 */
let work: Promise<void> | null = null;

/** Run `action` as the Import screen's work of the moment (`work`). */
function asWork(action: () => Promise<void>): Promise<void> {
  const tracked = action().finally(() => {
    if (work === tracked) work = null;
  });
  work = tracked;
  return tracked;
}

/**
 * Make the store `accountId`'s, before that account starts or resumes a run.
 *
 * A store already the account's is left as it is. A run another account
 * started is stopped and let settle first, so nothing it does afterwards
 * lands in the new run: its stage is cancelled, which leaves the run open on
 * the server for that account to resume from its run directory. Then the
 * store goes back to a fresh form, named with `accountId` (#1085).
 */
async function takeRunFor(accountId: number | null): Promise<void> {
  while (store.get().accountId !== accountId) {
    const running = work;
    if (running == null) {
      resetImportRun();
      store.set({ accountId });
      return;
    }
    await scratch.runCancel.cancel();
    await running.catch(() => {});
  }
}

/**
 * True when the account logged in now is not the one that started the run:
 * that account logged out while a stage ran. The server calls the run makes
 * go with the session logged in now, so the run must not make another.
 */
function accountLeft(): boolean {
  return getAccountId() !== store.get().accountId;
}

/**
 * Stop the run where it got to, the way a Cancel does, once the account that
 * started it has left. The run stays open for that account to resume.
 */
function stopIfAccountLeft(): void {
  if (accountLeft()) throw new Error(CANCELLED_MESSAGE);
}

function updateSteps(update: (steps: ImportStep[]) => ImportStep[]): void {
  store.set((state) => ({ steps: update(state.steps) }));
}

/** Mark whichever row is active as failed. */
function failActiveStep(): void {
  updateSteps((steps) =>
    steps.map((step) => (step.status === "active" ? { ...step, status: "error" } : step)),
  );
}

function setRowByLabel(label: string, patch: Partial<ImportStep>): void {
  updateSteps((steps) =>
    steps.map((step) => (step.label === label ? { ...step, ...patch } : step)),
  );
}

/**
 * Back to the form. The run's record stays on the server; only what the
 * screen holds goes. `resumeError` is kept on purpose (see the store).
 */
function returnToForm(): void {
  store.set({
    phase: "form",
    summaryView: null,
    stagingDir: null,
    importRunId: null,
    stagingSummary: null,
    mediaSummary: null,
    mediaFailedCount: null,
    mediaToolsMissing: false,
    mediaPartiallyRan: false,
    computingSummary: false,
    reviewError: null,
    form: null,
  });
}

/** Start a run's bookkeeping from nothing. */
function beginRun(form: ImportJobFormValues, firstStage: ImportIssueStage): void {
  scratch.importStartedAt = performance.now();
  scratch.activeStage = firstStage;
  scratch.issues = [];
  scratch.notes = [];
  scratch.conversations = new Map();
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
function currentPart(
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
    report,
  };
}

/**
 * Pick up what the run's earlier parts recorded, from its run directory. A
 * record that cannot be read leaves the run with only what this part
 * records: the resume itself goes ahead.
 */
async function loadCarriedRecord(stagingDir: string): Promise<void> {
  try {
    scratch.carried = parseRunRecord(await invokeReadImportRunRecord({ staging_dir: stagingDir }));
  } catch {
    scratch.carried = EMPTY_RUN_RECORD;
  }
}

/** A write of the run record waiting for the one before it to finish. */
type RecordWrite = { stagingDir: string; build: () => RunRecord };

/** Every write of the run record so far, in order: settles when the last has. */
let recordWrites: Promise<void> = Promise.resolve();

/** The write queued behind the one in progress, not yet started. */
let queuedRecordWrite: RecordWrite | null = null;

/**
 * Write a run record into its run directory, one write at a time and in the
 * order they were asked for, so an older record never lands over a newer one.
 * The record is built when its write starts, from the run as it stands then,
 * and a write asked for while another waits for the same directory replaces the
 * waiting one: issues that arrive while a write is in progress all go in the
 * next. A failed write is not shown: the run goes on, and a resume starts
 * from the record the directory already held.
 */
function writeRunRecord(stagingDir: string, build: () => RunRecord): Promise<void> {
  if (queuedRecordWrite?.stagingDir === stagingDir) {
    queuedRecordWrite.build = build;
    return recordWrites;
  }
  const write: RecordWrite = { stagingDir, build };
  queuedRecordWrite = write;
  recordWrites = recordWrites.then(async () => {
    if (queuedRecordWrite === write) queuedRecordWrite = null;
    try {
      await invokeSaveImportRunRecord({ staging_dir: write.stagingDir, record: write.build() });
    } catch {
      // Nothing to show; see above.
    }
  });
  return recordWrites;
}

/**
 * Write the run's record so far into its run directory, for the part
 * that resumes it. Called wherever the run stops with the run still open (at
 * a Review, and when `finishImport` leaves the run open), and while a stage
 * runs, as each issue arrives (`recordIssue`, `recordFileDone`). A failed
 * write loses only this part's record; the run itself is unaffected.
 *
 * While a stage runs, the write leaves in the run directory every issue
 * the window had received: each stage sends its issues the moment it
 * records them, and the Upload says when it has sent each conversation, so
 * a crash loses only what arrived while the last write was on its way to
 * disk. The record is the one a stop now would leave (`recordToCarry`): an
 * Upload's rows about a conversation not yet on the server wait apart, and
 * an earlier stop's rows about a conversation this Upload has since sent
 * go.
 */
async function saveCarriedRecord(
  report: UploadFinishedReport | null = null,
  uploadMs: number | null = null,
): Promise<void> {
  const { stagingDir } = store.get();
  if (stagingDir == null) return;
  const run = scratch;
  await writeRunRecord(stagingDir, () =>
    recordToCarry(run.carried, currentPart(report, uploadMs, run)),
  );
}

function applyProgress(event: ImportProgressEvent): void {
  const now = performance.now();

  scratch.timing = recordStageTime(scratch.timing, event.step, now);
  if (event.step === "parse") {
    scratch.counts.messagesParsed =
      event.total > 0 && event.done >= event.total ? event.total : event.done;
  } else if (event.step === "attachments") {
    scratch.lastAttachmentProgress = {
      done: event.done,
      total: event.total,
      bytesDone: event.bytes_done ?? 0,
      bytesTotal: event.bytes_total ?? 0,
    };
  }

  const stepIndex = stepIndexFor(event.step, scratch.attachmentMode);
  // No row for this step in the current mode (or an unrecognised step off
  // the wire): leave activeStage pointing at the stage of whatever step has a row, so a
  // dropped event here never mislabels the next error.
  if (stepIndex < 0) return;
  // An issue raised during setup is a problem reading the backup, which is
  // Staging in the Import Errors list.
  scratch.activeStage = stageForStep(event.step);

  const detail = rowDetail(event);
  const done = isProgressStepComplete(event.step, event.done, event.total);

  updateSteps((current) =>
    current.map((step, index) => {
      if (index < stepIndex) {
        return { ...step, status: "done" };
      }
      if (index > stepIndex) return step;
      return {
        ...step,
        status: done ? "done" : "active",
        detail,
      };
    }),
  );
}

/** The progress steps that report on the Staging row. */
type StagingProgressStep = Extract<
  ImportProgressEvent["step"],
  "setup" | "parse" | "attachments" | "prepare" | "check"
>;

/** The Staging row's steps in the order their lines show: conversations,
 * then messages, then attachments. */
const STAGING_LINE_ORDER: readonly StagingProgressStep[] = [
  "setup",
  "prepare",
  "parse",
  "attachments",
  "check",
];

function isStagingProgressStep(step: ImportProgressEvent["step"]): step is StagingProgressStep {
  return (STAGING_LINE_ORDER as readonly string[]).includes(step);
}

/**
 * The row's detail for one progress event. On the Staging row this event
 * updates its own line and leaves the others in place; the setup line goes
 * once messages are being read, since setup is over by then.
 */
function rowDetail(event: ImportProgressEvent): string {
  const line = progressDetail(event);
  if (!isStagingProgressStep(event.step)) return line;
  scratch.stagingLines = { ...scratch.stagingLines, [event.step]: line };
  if (event.step !== "setup") delete scratch.stagingLines.setup;
  return STAGING_LINE_ORDER.flatMap((step) => scratch.stagingLines[step] ?? []).join("\n");
}

/** The detail line for one progress event. */
function progressDetail(event: ImportProgressEvent): string {
  if (event.step === "setup") return setupDetail(event);
  if (event.step === "attachments") {
    const last = scratch.lastAttachmentProgress;
    return formatAttachmentProgress({
      mode: scratch.attachmentMode,
      done: event.done,
      total: event.total,
      bytesDone: event.bytes_done ?? last?.bytesDone ?? 0,
      bytesTotal: event.bytes_total ?? last?.bytesTotal ?? 0,
    });
  }
  const numbers = `${event.done.toLocaleString()}/${event.total.toLocaleString()}`;
  const counts = event.status ? `${numbers} (${event.status})` : numbers;
  return `${progressLabel(event.step, scratch.attachmentMode)}: ${counts}`;
}

function recordIssue(event: ImportIssueEvent): void {
  if (event.kind === "resolved") {
    // An earlier row no longer holds: it goes, and nothing replaces it.
    const resolved = { stage: stageForStep(event.step), item: event.item };
    scratch.issues = withoutResolved(scratch.issues, resolved);
    scratch.carried = resolveInRecord(scratch.carried, resolved);
  } else if (event.kind === "note") {
    // In place: an exporter may send a row per item, tens of thousands in a
    // run, and copying the list for each would cost the square of that.
    scratch.notes.push(noteFromEvent(event));
  } else {
    scratch.issues.push(issueFromEvent(event));
  }
  void saveCarriedRecord();
}

/**
 * Note what the Upload did with one conversation. The record changes only
 * when some row is about that conversation, so it is written again only
 * then.
 */
function recordFileDone(event: ImportFileDoneEvent): void {
  scratch.conversations.set(event.file, event.status);
  const about = (issue: ImportIssue) => issue.conversation === event.file;
  if (scratch.issues.some(about) || (scratch.carried.lastStopIssues ?? []).some(about)) {
    void saveCarriedRecord();
  }
}

function recordError(stage: ImportIssueStage, message: string): void {
  scratch.issues.push({ kind: "error", stage, item: RUN_ERROR_ITEM, reason: message });
}

/**
 * Run one desktop job to its end, feeding its progress and issues into the
 * run. A job that a Cancel came before is never started, and fails with
 * `CANCELLED_MESSAGE` the way a job cancelled while it runs does.
 */
function runJob(invokeFn: () => Promise<void>): Promise<TauriJobResult> {
  const guarded = scratch.runCancel.guard(invokeFn);
  return awaitTauriJob(
    "Import Run",
    async () => {
      stopIfAccountLeft();
      await guarded();
    },
    undefined,
    applyProgress,
    recordIssue,
    recordFileDone,
  );
}

/**
 * `invokeSummarizeStaging`, with a listener on the same `extract:progress`
 * channel the extract and media passes use. `summarize_staging` (Rust)
 * emits progress on the `check` step while it walks a big directory, and
 * `applyProgress` already knows to draw that on the Staging row, so this
 * only has to make sure the event reaches it.
 */
async function summarizeStagingWithProgress(config: StagingConfig): Promise<StagingSummary> {
  const unlisten = await onExtractEvents({
    onLog: () => {},
    onProgress: applyProgress,
    onFinished: () => {},
    onError: () => {},
  });
  try {
    return await invokeSummarizeStaging(config);
  } finally {
    unlisten();
  }
}

/**
 * A stage change the server did not record. The run stops where the server
 * last recorded it, so a later visit resumes it from there.
 */
class StageNotRecordedError extends Error {}

/**
 * Move a live run to another stage, carrying the summary the person just
 * approved when there is one. `approvedPlan` is simply forwarded, undefined
 * and all: `setImportStage` posts `{ stage, summary: approvedPlan }`, and
 * `JSON.stringify` drops an `undefined`-valued property outright, so an
 * omitted plan and an explicit `undefined` reach the server identically —
 * no `summary` key at all, leaving whatever plan is already stored untouched.
 *
 * Throws `StageNotRecordedError` when the write fails. A later visit resumes
 * the run from the stage the server holds, so the caller must not go on to
 * work the server does not know the run reached.
 */
async function moveStage(
  sessionId: number,
  stage: ImportStage,
  approvedPlan?: StagingSummary,
): Promise<void> {
  try {
    await setImportStage(sessionId, stage, approvedPlan);
  } catch (e: unknown) {
    const reason = e instanceof Error ? e.message : String(e);
    throw new StageNotRecordedError(`Message Crate didn't record the run's progress: ${reason}`);
  }
}

/**
 * Record that the run is waiting at a review. A failure leaves the review on
 * screen with the error on it (`reviewError`), and approving writes the stage
 * again before anything else (`approve`). Returns whether the server has it.
 */
async function moveStageAtReview(
  sessionId: number,
  stage: "staging_review" | "media_review",
  approvedPlan?: StagingSummary,
): Promise<boolean> {
  try {
    await moveStage(sessionId, stage, approvedPlan);
    store.set({ reviewError: null });
    return true;
  } catch (e: unknown) {
    store.set({ reviewError: e instanceof Error ? e.message : String(e) });
    return false;
  }
}

/** True when ffmpeg is needed for this mode and cannot be found. */
async function mediaToolsMissingFor(mode: AttachmentMediaMode): Promise<boolean> {
  if (mediaJobVerb(mode) === null) return false;
  try {
    const probe = await probeFfmpegTools(null);
    return !probe.ok;
  } catch {
    return true;
  }
}

/** Stop at a review: the run waits, and the review takes the screen. */
function waitAtReview(phase: "staging_review" | "media_review"): void {
  store.set({ phase, running: false, computingSummary: false });
}

/**
 * Build the run's summary, record it, and end the run or leave it open.
 *
 * A run ends when the server takes `/complete`, and its run directory is
 * deleted then: the server holds the record, and nothing will read the
 * directory again (#1233). That covers a finished Upload, and a failed Staging
 * or Media stage, which is discarded at once because nothing complete exists
 * to upload; ending it also frees the account to start a new import.
 *
 * Every other way out leaves the run open on the server, at the stage it
 * reached, with its directory and the record of this part in it
 * (`saveCarriedRecord`), and the next visit to Import offers it again:
 *
 * - `paused`: an Upload that did not send every conversation, by Pause or
 *   by failure (`importOutcome`). It posts no `/complete`.
 * - `skipComplete`: a cancelled Staging or Media stage, and a stage change
 *   the server did not record. Both resume from what is on disk; posting
 *   `/complete` would free the run's slot and strand the directory.
 * - A `/complete` the server refuses: the server still holds the run as
 *   running. A finished Upload then shows as paused, and its resume finds
 *   every message sent and posts `/complete` again.
 *
 * The summary and the completion cover the whole run, the earlier parts'
 * record (`scratch.carried`) included.
 */
async function finishImport(args: {
  sessionId: number | null;
  status: "completed" | "completed_with_issues" | "failed" | "cancelled" | "paused";
  uploadReport: UploadFinishedReport | null;
  uploadMs: number | null;
  skipComplete?: boolean;
}): Promise<void> {
  const { sessionId, status, uploadReport, uploadMs, skipComplete } = args;
  const carried = scratch.carried;
  const whole = wholeRun(carried, currentPart(uploadReport, uploadMs));
  const finalSummary: ImportSummaryView = {
    status,
    messagesParsed: whole.messagesParsed,
    filesTotal: uploadReport?.conversations_total ?? whole.filesParsed,
    filesSucceeded: whole.filesSucceeded,
    filesFailed: uploadReport?.conversations_failed,
    filesSkipped: uploadReport ? filesSkippedOverRun(carried, uploadReport) : undefined,
    messagesAttempted: whole.messagesAttempted,
    messagesInserted: whole.messagesInserted,
    messagesDeduped: whole.messagesDeduped,
    messagesFailed: uploadReport?.messages_failed,
    attachmentsUploaded: whole.attachmentsUploaded,
    parseMs: whole.parseMs,
    attachmentsMs: whole.attachmentsMs,
    prepareMs: whole.prepareMs,
    uploadMs: whole.uploadMs,
    durationMs: whole.durationMs ?? null,
    issues: whole.issues,
    notes: whole.notes ?? [],
  };
  // Keyed by label: the Staging row folds reading, attachments and prepare
  // into one duration, and a mode with no Media stage has fewer rows.
  const { parseMs, attachmentsMs, prepareMs } = whole;
  const stagingMs =
    parseMs != null || attachmentsMs != null || prepareMs != null
      ? (parseMs ?? 0) + (attachmentsMs ?? 0) + (prepareMs ?? 0)
      : null;
  const durationByLabel = new Map<string, number | null>([
    [STAGING_LABEL, stagingMs],
    [UPLOAD_LABEL, whole.uploadMs ?? null],
  ]);
  updateSteps((current) =>
    current.map((step) => {
      const duration = durationByLabel.get(step.label);
      if (duration == null) return step;
      return { ...step, durationMs: duration };
    }),
  );
  const posts = sessionId != null && !skipComplete && status !== "paused";
  let completeRefused: string | null = null;
  if (posts) {
    try {
      // The server counts the messages and attachments the run holds: a
      // resumed Upload's report counts only what the resume sent.
      await completeImport(sessionId, {
        status,
        bytes_uploaded: whole.bytesUploaded,
        parse_ms: whole.parseMs,
        attachments_ms: whole.attachmentsMs,
        prepare_ms: whole.prepareMs,
        upload_ms: whole.uploadMs,
        duration_ms: whole.durationMs,
        summary: {
          files_total: finalSummary.filesTotal,
          files_succeeded: finalSummary.filesSucceeded,
          files_failed: finalSummary.filesFailed,
          files_skipped: finalSummary.filesSkipped,
          messages_parsed: finalSummary.messagesParsed,
          messages_attempted: finalSummary.messagesAttempted,
          messages_inserted: finalSummary.messagesInserted,
          messages_deduped: finalSummary.messagesDeduped,
          messages_failed: finalSummary.messagesFailed,
        },
        issues: issueRequests(finalSummary.issues),
        notes: whole.notes ?? [],
      });
    } catch (e: unknown) {
      completeRefused = e instanceof Error ? e.message : String(e);
    }
  }
  if (completeRefused != null) {
    if (status === "completed" || status === "completed_with_issues") {
      finalSummary.status = "paused";
      setRowByLabel(UPLOAD_LABEL, { status: "error", detail: "Paused" });
    }
    // Shown here only, since the server never took the issues it would be
    // recorded with.
    finalSummary.issues = [
      ...finalSummary.issues,
      {
        kind: "error",
        stage: "upload",
        item: RUN_ERROR_ITEM,
        reason: `Message Crate didn't record the import as finished: ${completeRefused}`,
      },
    ];
  }
  // A run with no server record at all (its creation failed) is ended too:
  // nothing will ever offer its directory again.
  const runEnded = sessionId == null || (posts && completeRefused == null);
  let stagingDir = store.get().stagingDir;
  if (runEnded) {
    // An ended run's directory goes: the staged messages, the push log, journal
    // and report, and the run record. When the delete fails, the directory link
    // stays and the failure is shown, so the person can find what was left
    // and remove it by hand. A record write still on its way finishes
    // first, so it cannot land in the directory after the delete.
    await recordWrites;
    if (stagingDir != null && (await discardStagingDirectory(stagingDir))) stagingDir = null;
  } else {
    await saveCarriedRecord(uploadReport, uploadMs);
  }
  // The server writes this run's saved search and Contact Group when the run
  // completes, so a window closed mid-import still gets them.
  store.set({ summaryView: finalSummary, phase: "done", running: false, stagingDir });
}

/**
 * Delete a run directory of a run that has ended or been discarded. Never
 * throws: a refusal or failed delete is kept on `stagingDeleteFailure` for
 * the screen to show. Returns whether the directory is gone.
 */
async function discardStagingDirectory(stagingDir: string): Promise<boolean> {
  try {
    await invokeDeleteStaging({ staging_dir: stagingDir });
    store.set((state) =>
      state.stagingDeleteFailure?.path === stagingDir ? { stagingDeleteFailure: null } : {},
    );
    return true;
  } catch (e: unknown) {
    store.set({
      stagingDeleteFailure: {
        path: stagingDir,
        reason: e instanceof Error ? e.message : String(e),
      },
    });
    return false;
  }
}

/** The person has read that a run directory was left behind. */
function dismissStagingDeleteFailure(): void {
  store.set({ stagingDeleteFailure: null });
}

/**
 * Upload to the server and record the outcome: the tail end shared by a
 * resumed run (jumps straight here), the Staging Review when there is no
 * Media stage, and the Media Review. Never throws: a push failure is
 * folded into the finished summary via `finishImport`, exactly like any
 * other terminal outcome.
 */
async function runUpload(
  token: string | null,
  sessionId: number,
  outputDir: string,
  approvedPlan?: StagingSummary,
): Promise<void> {
  // Logging out pauses this Upload before it ends the session the push
  // sends (`lib/runningUpload.ts`), and waits until the pause is recorded.
  const runCancel = scratch.runCancel;
  const upload = uploadAndFinish(token, sessionId, outputDir, approvedPlan);
  const pause = async () => {
    await runCancel.cancel();
    await upload;
  };
  const ended = registerRunningUpload(pause);
  let sessionRefused = false;
  try {
    sessionRefused = await upload;
  } finally {
    ended();
  }
  // The push stopped because the server refused its session: every request
  // with that token is refused now, so the session ends here too (#1491).
  if (sessionRefused && token) uploadSessionRefused(token);
}

/**
 * `runUpload` without the registration that lets logging out pause it.
 * Resolves to whether the server refused the session the push sent.
 */
async function uploadAndFinish(
  token: string | null,
  sessionId: number,
  outputDir: string,
  approvedPlan?: StagingSummary,
): Promise<boolean> {
  store.set({ running: true, phase: "running" });
  scratch.activeStage = "upload";
  setRowByLabel(UPLOAD_LABEL, { status: "active", detail: "Uploading to Message Crate…" });
  try {
    await moveStage(sessionId, "upload", approvedPlan);
  } catch (e: unknown) {
    // The server still has the run at its review, so the run stays there
    // and is not completed: a later visit offers that review again.
    recordError("upload", e instanceof Error ? e.message : String(e));
    failActiveStep();
    await finishImport({
      sessionId,
      status: "failed",
      uploadReport: null,
      uploadMs: null,
      skipComplete: true,
    });
    return false;
  }

  const uploadStartedAt = performance.now();
  let uploadResult: TauriJobResult | null = null;
  let threw = false;
  // A Pause that came before the push started: the guard refused the job.
  let pausedBeforeStart = false;
  try {
    const baseUrl = getBaseUrl();
    if (!token) throw new Error("Not authenticated");
    uploadResult = await runJob(() =>
      invokeUpload({
        base_url: baseUrl,
        username: "",
        token,
        input_dir: outputDir,
        mode: "append",
        skip_attachments: false,
        // Extract (or the Media stage) just wrote these files. Matching
        // size_bytes lets message-crate-push skip a second full-file hash.
        trust_export: true,
        import_id: sessionId,
      }),
    );
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    if (msg === CANCELLED_MESSAGE) {
      // The person asked for this: not an error, so no issue row for it.
      pausedBeforeStart = true;
    } else {
      threw = true;
      recordError(scratch.activeStage, msg);
      failActiveStep();
    }
  }
  const uploadMs = performance.now() - uploadStartedAt;
  const report = uploadResult?.report ?? null;
  // An Upload that did not send every conversation, paused or failed, is
  // paused: the run stays at `upload` with its directory, and resuming it
  // sends only what the push journal does not list.
  const status = pausedBeforeStart
    ? "paused"
    : importOutcome({
        report: report ?? undefined,
        threw,
        issues: wholeRun(scratch.carried, currentPart(report, uploadMs)).issues,
        approved: approvedPlan,
      });
  if (status === "paused") {
    setRowByLabel(UPLOAD_LABEL, { status: "error", detail: "Paused", durationMs: uploadMs });
  } else {
    setRowByLabel(UPLOAD_LABEL, {
      status: "done",
      detail: "Upload complete",
      durationMs: uploadMs,
    });
  }

  await finishImport({ sessionId, status, uploadReport: report, uploadMs });
  return report?.session_refused === true;
}

/**
 * Convert or compress the staged files after the Staging Review, then
 * recompute the summary against the directory as it now stands (the directory is
 * the truth, not the last estimate) and stop at the Media Review. A
 * failed stage ends the import as failed and deletes its directory, never a
 * silent fall-through to Upload. A failed recompute after a stage that
 * succeeded returns to the form and keeps the directory.
 *
 * `approvedSummary` is undefined on a resume whose stored plan failed to
 * parse (`parseStoredStagingSummary`): `moveStage` tolerates that absence,
 * so the stage still runs rather than blocking
 * the resume over a plan that can no longer be read.
 */
async function runMediaPass(
  form: ImportJobFormValues,
  sessionId: number,
  outputDir: string,
  approvedSummary?: StagingSummary,
): Promise<void> {
  store.set({ running: true, phase: "running" });
  scratch.activeStage = "media";
  setRowByLabel(MEDIA_LABEL, { status: "active", detail: `${mediaVerb(form.attachmentMedia)}…` });

  // Carries the plan approved at the Staging Review even on this stage: a
  // crash mid-pass must not leave `summary_json` null with no baseline for
  // a later resume to diff against.
  try {
    await moveStage(sessionId, "media", approvedSummary);
  } catch (e: unknown) {
    // The server still has the run at the Staging Review, so the run stays
    // there and is not completed: a later visit offers that review again.
    recordError("media", e instanceof Error ? e.message : String(e));
    failActiveStep();
    await finishImport({
      sessionId,
      status: "failed",
      uploadReport: null,
      uploadMs: null,
      skipComplete: true,
    });
    return;
  }

  const mediaStartedAt = performance.now();
  let transcodeReport: TranscodeFinishedReport | undefined;
  let threw = false;
  let cancelled = false;
  try {
    const result = await runJob(() => invokeTranscodeStaging({ staging_dir: outputDir }));
    transcodeReport = result.transcode;
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    if (msg === CANCELLED_MESSAGE) {
      // The person asked for this: not an error, so no issue row for it.
      cancelled = true;
    } else {
      threw = true;
      recordError(scratch.activeStage, msg);
    }
  }
  const mediaMs = performance.now() - mediaStartedAt;

  if (threw || cancelled) {
    failActiveStep();
    // Neither path writes another stage: the run stays at `media`,
    // which is exactly where it got to. A cancellation also skips
    // `/complete` outright (see finishImport), so the run stays running and
    // resumable instead of completing and freeing the slot out from under a
    // run directory nobody can reach any more. A failed stage is discarded:
    // it completes as failed and its directory goes, so a broken ffmpeg does
    // not lock the account out of importing.
    await finishImport({
      sessionId,
      status: cancelled ? "cancelled" : "failed",
      uploadReport: null,
      uploadMs: null,
      skipComplete: cancelled,
    });
    return;
  }

  setRowByLabel(MEDIA_LABEL, {
    status: "done",
    detail: mediaDoneDetail(form.attachmentMedia),
    durationMs: mediaMs,
  });

  store.set({ computingSummary: true });
  await moveStageAtReview(sessionId, "media_review", approvedSummary);
  // Media's times are only in memory until now, and the run may be resumed
  // from this Review after the app closes.
  await saveCarriedRecord();
  try {
    const actual = await summarizeStagingWithProgress({ staging_dir: outputDir });
    store.set({ mediaSummary: actual, mediaFailedCount: transcodeReport?.failed ?? null });
    waitAtReview("media_review");
  } catch (e: unknown) {
    // Media itself succeeded; only reading the directory afterwards failed.
    // The converted directory is the run's work, so the run is not completed
    // and its directory stays: back to the form, as after Staging, with the
    // failure on `resumeError`. The run waits at the Media Review on the
    // server, and resuming it there reads the directory again.
    store.set({
      resumeError: e instanceof Error ? e.message : String(e),
      computingSummary: false,
      running: false,
    });
    returnToForm();
  }
}

/**
 * Fields extract needs for this form's source. The media fields go only to
 * a source whose form shows them, as the person chose them: extract checks
 * them before anything is staged and records them for the later stages.
 */
function extractFieldsFor(form: ImportJobFormValues) {
  const media = {
    attachmentMedia: form.attachmentMedia,
    maxResolution: form.maxResolution,
    maxFps: form.maxFps,
    minSizeMb: form.minSizeMb,
  };
  if (isImessageMethod(form.source)) {
    return imessageExtractFields({
      source: form.source,
      backupPassword: form.backupPassword,
      ...media,
      obfuscate: form.obfuscate,
      attachmentRoot: form.attachmentRoot,
      appleContacts: form.appleContacts,
    });
  }
  if (isWhatsappMethod(form.source)) {
    return whatsappExtractFields({
      source: form.source,
      ...media,
      key: form.whatsappKey,
      backupPassword: form.backupPassword,
      wa: form.whatsappWa,
      media: form.whatsappMedia,
      db: form.whatsappDb,
      business: form.whatsappBusiness,
      ownerPhone: form.whatsappOwnerPhone,
    });
  }
  if (form.isAndroidSms) {
    return sbrExtractFields({
      ...media,
      ownerPhones: form.ownerPhones,
      ownerEmails: form.ownerEmails,
      obfuscate: form.obfuscate,
    });
  }
  if (form.source === IMAZING_SOURCE_ID) {
    return { timezone: form.timeZone };
  }
  return {};
}

async function runImport(
  token: string | null,
  submitted: ImportJobFormValues,
  identities: string[] | null,
  resume?: ResumeUpload,
  resumeWrite?: ResumeWrite,
): Promise<void> {
  if (!isTauri()) return;
  let form = withShownAttachmentMode(submitted);
  beginRun(form, "staging");
  store.set({
    running: true,
    phase: "running",
    form,
    summaryView: null,
    stagingDir: null,
    importRunId: null,
    stagingSummary: null,
    mediaSummary: null,
    mediaFailedCount: null,
    mediaToolsMissing: false,
    mediaPartiallyRan: false,
    resumeError: null,
    reviewError: null,
    computingSummary: false,
  });

  let sessionId: number | null = null;

  try {
    if (!token) throw new Error("Not authenticated");

    if (!resume && !resumeWrite) {
      // A new Import Run works to the server's attachment size limit as it is
      // now. It goes into the form the run is created with, so every later
      // stage, and a resume, measures against this same number.
      const server = await getServerState();
      form = { ...form, assetMaxBytes: server.asset_max_bytes };
      scratch.form = form;
      store.set({ form });
    }

    if (resume) {
      // The run directory is already complete, so there is nothing to
      // resolve, no new run to create (the account already has this one),
      // and no extract to run. resume_upload is only ever offered after the
      // last review, so there IS a plan from it: it rides along as
      // `resume.approved` (parsed from the run's stored summary) when it
      // parses. Straight to Upload.
      const outputDir = resume.stagingDir;
      sessionId = resume.sessionId;
      await loadCarriedRecord(outputDir);
      // The approved plan was read from the directory at its review, so it
      // carries the mode Staging recorded there.
      form = adoptRecordedMode(form, resume.approved);
      store.set({
        stagingDir: outputDir,
        importRunId: sessionId,
        steps: stepsFor(form.attachmentMedia).map((step) =>
          step.label === UPLOAD_LABEL
            ? { ...step, status: "active", detail: "Uploading to Message Crate…" }
            : { ...step, status: "done", detail: "Already staged" },
        ),
      });
      await runUpload(token, sessionId, outputDir, resume.approved);
      return;
    }

    // Only a run that extracts starts from the fresh list; the resume above
    // built its own, so setting this first would be overwritten.
    store.set({ steps: initialSteps("active", form.attachmentMedia) });

    let outputDir: string;
    if (resumeWrite) {
      // The run already exists and its Staging was interrupted. Reuse it and
      // its run directory: the exporter reads the backup again and skips
      // the conversations already written.
      outputDir = resumeWrite.stagingDir;
      sessionId = resumeWrite.sessionId;
      await loadCarriedRecord(outputDir);
      store.set({ stagingDir: outputDir, importRunId: sessionId });
      setRowByLabel(STAGING_LABEL, { detail: "Extracting…" });
      await moveStage(sessionId, "write");
    } else {
      outputDir = await invokeCreateStagingDir(form.source);
      store.set({ stagingDir: outputDir });

      const backupStat = await invokePathStat(form.backupPath).catch(() => null);
      // The run is created in the account of the session logged in now.
      stopIfAccountLeft();
      const importSession = await createImport({
        ...importRunCreateBody(form.source),
        stage: "parse",
        staging_dir: outputDir,
        device_id: getDeviceId(),
        form: formSnapshot(form),
        source_fingerprint: backupStat ? buildSourceFingerprint(form.backupPath, backupStat) : null,
        source_identities: identities,
      });
      sessionId = importSession.id;
      store.set({ importRunId: sessionId });
      setRowByLabel(STAGING_LABEL, { detail: "Extracting…" });
      await moveStage(sessionId, "write");
    }

    scratch.timing.extractStartedAt = performance.now();
    const extractResult = await runJob(() =>
      invokeExtract({
        source: form.source,
        path: form.backupPath,
        output_dir: outputDir,
        ...(resumeWrite ? { resume: true } : {}),
        asset_max_bytes: assetLimitOf(form),
        ...extractFieldsFor(form),
      }),
    );
    if (extractResult.extraction) {
      scratch.counts.filesParsed = extractResult.extraction.files_parsed;
      scratch.counts.messagesParsed = extractResult.extraction.messages_parsed;
    }

    const extractFinishedAt = performance.now();
    const { parseMs, attachmentsMs, prepareMs } = stageDurations(scratch.timing, extractFinishedAt);
    scratch.durations = { parseMs, attachmentsMs, prepareMs };
    // What extract did, as the Staging row's done line.
    const attachmentDoneLine = attachmentDoneDetail(
      form.attachmentMedia,
      scratch.lastAttachmentProgress,
    );
    store.set({
      steps: stepsFor(form.attachmentMedia).map((step) =>
        step.label === STAGING_LABEL
          ? {
              ...step,
              status: "done" as const,
              detail: attachmentDoneLine,
              durationMs: parseMs + attachmentsMs + prepareMs,
            }
          : // Media and Upload: not run yet, the Staging Review comes first.
            step,
      ),
      computingSummary: true,
    });

    await moveStageAtReview(sessionId, "staging_review");
    // Staging's issues and times are only in memory until now, and the run
    // may be resumed from this Review after the app closes.
    await saveCarriedRecord();
    // The extract itself is done and staged: an error from here on is a
    // failed read of a directory that already holds the staged work, not a run
    // that failed. Routing it through the outer catch (below) would post
    // `/complete` and end the run, stranding that work with no way back to
    // it. This mirrors `resumeAtReview`'s landing exactly: return to the form
    // instead, surfacing the failure on `resumeError`. The stage already
    // written above (`staging_review`) stays as it is: the next visit's
    // resume check finds the same run and offers this recompute again.
    try {
      const summary = await summarizeStagingWithProgress({ staging_dir: outputDir });
      // Staging has finished, so the directory decides the stages from here.
      const recorded = adoptRecordedMode(form, summary);
      updateSteps((steps) =>
        stepsFor(recorded.attachmentMedia).map(
          (step) => steps.find((row) => row.label === step.label) ?? step,
        ),
      );
      const toolsMissing = await mediaToolsMissingFor(recorded.attachmentMedia);
      store.set({ stagingSummary: summary, mediaToolsMissing: toolsMissing });
      waitAtReview("staging_review");
    } catch (e: unknown) {
      store.set({
        resumeError: e instanceof Error ? e.message : String(e),
        computingSummary: false,
        running: false,
      });
      returnToForm();
    }
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    // A cancelled Staging is not a failure: the conversations already
    // written are real work, and Staging can pick up from them. Leaving the
    // run at `write` is what lets the next Import visit offer that. A
    // `write` stage the server did not record stops the run where the server
    // has it, so it is not completed either. A genuine failure still
    // completes: a broken backup must not lock the account out of importing.
    const cancelled = msg === CANCELLED_MESSAGE;
    const stageNotRecorded = e instanceof StageNotRecordedError;
    if (!cancelled) recordError(scratch.activeStage, msg);
    failActiveStep();
    store.set({ computingSummary: false });
    await finishImport({
      sessionId,
      status: cancelled ? "cancelled" : "failed",
      uploadReport: null,
      uploadMs: null,
      skipComplete: cancelled || stageNotRecorded,
    });
  }
}

/**
 * End a run the person gave up on, by a Cancel at a Review or a Discard of a
 * paused run: close it on the server as cancelled, with the Import Errors and
 * notes its record holds (`issuesToDiscard`, `notesToDiscard`), and delete
 * its run directory.
 *
 * The record is in the directory, so it is read before the directory goes, and a
 * record that cannot be read discards the run with no Import Errors. The
 * close and the delete then run regardless of the other's outcome: a live
 * run with no directory blocks the next import, and a directory with no run is
 * litter nothing will ever clean up. `stagingDir` is null for a run whose
 * directory is not on this device. Never throws.
 */
async function discardRun(sessionId: number | null, stagingDir: string | null): Promise<void> {
  let issues: ImportIssue[] = [];
  let notes: ImportNote[] = [];
  if (stagingDir != null) {
    await recordWrites;
    try {
      const record = parseRunRecord(await invokeReadImportRunRecord({ staging_dir: stagingDir }));
      issues = issuesToDiscard(record);
      notes = notesToDiscard(record);
    } catch {
      // Discarded with no Import Errors: the run still has to close.
    }
  }
  await Promise.allSettled([
    sessionId != null
      ? discardImportRun(sessionId, issueRequests(issues), notes)
      : Promise.resolve(),
    stagingDir != null ? discardStagingDirectory(stagingDir) : Promise.resolve(),
  ]);
}

/** Cancel the run at a Review: end it (`discardRun`) and go back to the form. */
async function cancelRun(): Promise<void> {
  if (scratch.reviewAction) return;
  scratch.reviewAction = true;
  try {
    const { importRunId: sessionId, stagingDir: outputDir } = store.get();
    await discardRun(sessionId, outputDir);
  } finally {
    scratch.reviewAction = false;
  }
  returnToForm();
}

/** Stop the stage that is running. The run stays where it got to. */
async function cancel(): Promise<void> {
  await scratch.runCancel.cancel();
}

/**
 * Run an import through its stages and reviews, and keep the run's
 * state where the screen can read it (`importRunStore`).
 *
 * Every function here reads the run from the store at the moment it is
 * called, never from a render's closure, so a screen that unmounted while a
 * stage ran and mounted again finds the run where it left it.
 */
export function useImportJob() {
  const fetchAccountProfile = useFetchAccountProfile();
  const auth = useAuth();
  const token = auth.token;
  const accountId = auth.accountId ?? null;
  // The logged-in account's run, or the fresh form when another account's
  // run is in the store (#1085).
  const state: ImportRunState = useImportRunState();

  /** True when the run in the store is this account's to act on. */
  function ownsRun(): boolean {
    return store.get().accountId === accountId;
  }

  /**
   * Start an import. For a fresh iMessage start this first reads which
   * addresses the backup's device sent from and compares them to the
   * profile; when nothing matches, it parks the form and stops at
   * `identity_stop`, before any run exists, so Cancel has nothing to clean
   * up. The probe fails open: a source it cannot read will fail in the
   * extractor moments later with the proper error.
   */
  async function startImport(
    form: ImportJobFormValues,
    resume?: ResumeUpload,
    resumeWrite?: ResumeWrite,
  ): Promise<void> {
    if (!isTauri()) return;
    await takeRunFor(accountId);
    // A second call while one is already probing or running is a no-op.
    if (scratch.startImport) return;
    scratch.startImport = true;
    const started = scratch;
    await asWork(async () => {
      try {
        await startRun(form, resume, resumeWrite);
      } finally {
        started.startImport = false;
      }
    });
  }

  /** `startImport`, once the store is this account's and no other start is under way. */
  async function startRun(
    form: ImportJobFormValues,
    resume?: ResumeUpload,
    resumeWrite?: ResumeWrite,
  ): Promise<void> {
    let identities: string[] | null = null;
    if (!resume && !resumeWrite && isImessageMethod(form.source)) {
      // The probe reads the backup (and, for an encrypted one, decrypts
      // it) before any run exists, which can take seconds: mark the run
      // busy for that stretch so the Import button reflects it.
      store.set({ running: true });
      try {
        identities = await invokeImessageBackupIdentities({
          path: form.backupPath,
          ios: form.source === "imessage-ios",
          backupPassword: form.backupPassword,
        }).catch(() => []);
        store.set({ sourceIdentities: identities });
        const profile = await fetchAccountProfile();
        // The account logged out during the probe: the profile just read
        // is another account's, and no run exists yet to resume.
        if (accountLeft()) return;
        if (needsIdentityStop(identities, profile)) {
          scratch.pendingIdentityForm = form;
          store.set({ phase: "identity_stop" });
          return;
        }
      } finally {
        store.set({ running: false });
      }
    } else {
      store.set({ sourceIdentities: resumeWrite ? (resumeWrite.identities ?? null) : null });
    }
    await runImport(token, form, identities, resume, resumeWrite);
  }

  /** Continue past the identity stop with the parked form. */
  async function continueAfterIdentityStop(): Promise<void> {
    if (!ownsRun()) return;
    const form = scratch.pendingIdentityForm;
    if (!form) return;
    scratch.pendingIdentityForm = null;
    await asWork(() => runImport(token, form, store.get().sourceIdentities));
  }

  /** Leave the identity stop; nothing was created, so only the phase moves. */
  function cancelIdentityStop(): void {
    if (!ownsRun()) return;
    scratch.pendingIdentityForm = null;
    returnToForm();
  }

  /** Approve the waiting review: Media after the Staging Review when there is one, Upload otherwise. */
  async function approve(): Promise<void> {
    if (!isTauri()) return;
    if (!ownsRun()) return;
    if (scratch.reviewAction) return;
    const form = scratch.form;
    const {
      phase,
      importRunId: sessionId,
      stagingDir: outputDir,
      stagingSummary,
      mediaSummary,
      reviewError,
    } = store.get();
    // What the person is approving: the directory as Media left it at the
    // Media Review, as Staging left it at the Staging Review.
    const approvedSummary = phase === "media_review" ? mediaSummary : stagingSummary;
    if (!form || sessionId == null || outputDir == null || approvedSummary == null) return;

    scratch.reviewAction = true;
    scratch.runCancel = createRunCancel();
    const approving = scratch;
    await asWork(async () => {
      try {
        // The review's own stage did not reach the server, so it is written
        // first: a later visit must find the run at this review.
        if (reviewError != null) {
          const recorded =
            phase === "media_review"
              ? await moveStageAtReview(sessionId, "media_review", stagingSummary ?? undefined)
              : await moveStageAtReview(sessionId, "staging_review");
          if (!recorded) return;
        }
        if (phase === "staging_review" && mediaJobVerb(form.attachmentMedia) !== null) {
          await runMediaPass(form, sessionId, outputDir, approvedSummary);
        } else {
          await runUpload(token, sessionId, outputDir, approvedSummary);
        }
      } finally {
        approving.reviewAction = false;
      }
    });
  }

  /**
   * Resume a run the server reports waiting at a review (`staging_review`
   * / `media_review`) or mid Media (`media`).
   *
   * `approve` can't do this itself: it depends on what the store holds
   * (`stagingSummary`, the form, `stagingDir`, `importRunId`) that a
   * reload has none of, and it branches on the phase rather than the run's
   * own stored stage. This rebuilds that state from `session` instead, then
   * routes exactly the way the normal flow would have got here.
   *
   * `resumedForm` is the caller's already-validated `restoreFormFromSnapshot`
   * result: the caller needs that check anyway (to fall back to
   * `settings_unreadable`), so this trusts it rather than parsing
   * `session.form` a second time.
   *
   * The directory is the truth. Every landing recomputes the summary fresh
   * from the run directory; the run's stored `summary` is read only as the
   * approved plan, for the Staging row on the Media Review and the Media
   * stage's own bookkeeping.
   *
   * A recompute failing here is a transient read of the run directory, not
   * a run that failed: only an explicit cancel ends a waiting run, so this
   * must not complete it or write a stage. It returns to the form instead
   * (the resume check there re-runs and finds the same run, so the panel
   * reappears; that is the retry) and leaves the failure on `resumeError`.
   */
  async function resumeAtReview(
    session: ActiveImportRun,
    resumedForm: ImportJobFormValues,
  ): Promise<void> {
    if (!isTauri()) return;
    await takeRunFor(accountId);
    await asWork(() => resumeRunAtReview(session, resumedForm));
  }

  /** `resumeAtReview`, once the store is this account's. */
  async function resumeRunAtReview(
    session: ActiveImportRun,
    resumedForm: ImportJobFormValues,
  ): Promise<void> {
    if (
      session.stage !== "staging_review" &&
      session.stage !== "media_review" &&
      session.stage !== "media"
    ) {
      return;
    }
    if (!session.staging_dir) return; // resumeDecisionFor guarantees this; defensive only.

    const sessionId = session.id;
    const outputDir = session.staging_dir;
    const approved = parseStoredStagingSummary(session.summary);
    // Staging has finished for every stage resumed here, so the mode comes
    // from the directory: through the plan approved at the Staging Review until
    // the summary below is recomputed from the directory itself.
    const known = withRecordedMode(resumedForm, approved);

    beginRun(known, session.stage === "media" ? "media" : "staging");
    await loadCarriedRecord(outputDir);
    store.set({
      resumeError: null,
      reviewError: null,
      form: known,
      summaryView: null,
      stagingDir: outputDir,
      importRunId: sessionId,
      stagingSummary: null,
      mediaSummary: null,
      mediaFailedCount: null,
      mediaToolsMissing: false,
      mediaPartiallyRan: false,
      sourceIdentities: parseSourceIdentities(session.source_identities),
    });

    /** Recompute the summary from the directory, then land on the given review. */
    async function landOn(
      review: "staging_review" | "media_review",
      partiallyRan: boolean,
    ): Promise<void> {
      const mediaDone = review === "media_review";
      store.set({
        steps: resumeSteps(known.attachmentMedia, mediaDone),
        computingSummary: true,
        phase: "running",
        running: true,
      });
      try {
        const actual = await summarizeStagingWithProgress({ staging_dir: outputDir });
        const recorded = adoptRecordedMode(known, actual);
        store.set({ steps: resumeSteps(recorded.attachmentMedia, mediaDone) });
        if (review === "staging_review") {
          const missing = await mediaToolsMissingFor(recorded.attachmentMedia);
          store.set({
            stagingSummary: actual,
            mediaToolsMissing: missing,
            mediaPartiallyRan: partiallyRan,
          });
        } else {
          // The Staging row shows the plan approved before Media, read
          // back from the run; the Media rows show the directory as it is now.
          // Media's own report is gone on a resume, so its failed count is
          // unknown rather than zero.
          store.set({
            stagingSummary: approved ?? null,
            mediaSummary: actual,
            mediaFailedCount: null,
          });
        }
        waitAtReview(review);
      } catch (e: unknown) {
        store.set({
          resumeError: e instanceof Error ? e.message : String(e),
          computingSummary: false,
          running: false,
        });
        returnToForm();
      }
    }

    if (session.stage === "staging_review") {
      await landOn("staging_review", false);
      return;
    }
    if (session.stage === "media_review") {
      await landOn("media_review", false);
      return;
    }

    // transcode: Media died mid-run. Re-running it is safe (the stage is
    // resumable), so long as the tools it needs are there: a resume with
    // ffmpeg missing falls back to the Staging Review's recomputed
    // summary instead of starting a job that can only fail, using the same
    // `mediaToolsMissing` gate the normal flow shows there.
    if (await mediaToolsMissingFor(known.attachmentMedia)) {
      await landOn("staging_review", true);
      return;
    }
    store.set({ steps: resumeSteps(known.attachmentMedia, false) });
    await runMediaPass(known, sessionId, outputDir, approved);
  }

  return {
    phase: state.phase,
    steps: state.steps,
    running: state.running,
    form: state.form,
    summaryView: state.summaryView,
    stagingDir: state.stagingDir,
    importRunId: state.importRunId,
    stagingSummary: state.stagingSummary,
    mediaSummary: state.mediaSummary,
    mediaFailedCount: state.mediaFailedCount,
    mediaToolsMissing: state.mediaToolsMissing,
    mediaPartiallyRan: state.mediaPartiallyRan,
    resumeError: state.resumeError,
    reviewError: state.reviewError,
    computingSummary: state.computingSummary,
    completionText:
      state.phase === "done" ? completionTextFor(state.summaryView?.status) : undefined,
    sourceIdentities: state.sourceIdentities,
    stagingDeleteFailure: state.stagingDeleteFailure,
    discardRun,
    dismissStagingDeleteFailure,
    startImport,
    continueAfterIdentityStop,
    cancelIdentityStop,
    approve,
    // Another account's run is that account's to cancel, stop or leave.
    cancelRun: async () => {
      if (ownsRun()) await asWork(cancelRun);
    },
    resumeAtReview,
    cancel: async () => {
      if (ownsRun()) await cancel();
    },
    returnToForm: () => {
      if (ownsRun()) returnToForm();
    },
  };
}
