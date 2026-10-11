import type { ImportIssue } from "../../../components/import/ImportSummaryPanel";
import type { ImportIssueStage } from "../../../components/import/importIssueStage";
import { formatAttachmentProgress } from "../../../lib/attachmentProgressCopy";
import {
  awaitDesktopJob,
  type DesktopJobResult,
  invokeSummarizeStaging,
  onDesktopJobEvents,
  type RunDirConfig,
  type StagingSummary,
} from "../../../lib/tauri";
import { waitingLine } from "../../../lib/toolStatusCopy";
import type {
  AttachmentMediaMode,
  ImportFileDoneEvent,
  ImportFileWrittenEvent,
  ImportIssueEvent,
  ImportProgressEvent,
} from "../../../lib/types";
import {
  isProgressStepComplete,
  issueFromEvent,
  noteFromEvent,
  recordStageTime,
  setupDetail,
  stageForStep,
  stepIndexFor,
} from "../importProgressState";
import {
  isStagingRowOfConversation,
  RUN_ERROR_ITEM,
  resolveInRecord,
  withoutResolved,
} from "../runRecord";
import { saveCarriedRecord } from "./runRecordWrites";
import { runScratch } from "./scratch";
import { stopIfAccountLeft } from "./serverCalls";
import { mediaVerb, updateSteps } from "./steps";

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

function applyProgress(event: ImportProgressEvent): void {
  const scratch = runScratch();
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
export type StagingProgressStep = Extract<
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
  const scratch = runScratch();
  const line = progressDetail(event);
  if (!isStagingProgressStep(event.step)) return line;
  scratch.stagingLines = { ...scratch.stagingLines, [event.step]: line };
  if (event.step !== "setup") delete scratch.stagingLines.setup;
  return STAGING_LINE_ORDER.flatMap((step) => scratch.stagingLines[step] ?? []).join("\n");
}

/** The detail line for one progress event. */
function progressDetail(event: ImportProgressEvent): string {
  const scratch = runScratch();
  // A run waiting for a download, on the Setup or the Media row, says only that.
  if (event.waiting) {
    return waitingLine(event.waiting, event.bytes_done ?? 0, event.bytes_total ?? null);
  }
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
  const scratch = runScratch();
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
  const scratch = runScratch();
  scratch.conversations.set(event.file, event.status);
  const about = (issue: ImportIssue) => issue.conversation === event.file;
  if (scratch.issues.some(about) || (scratch.carried.lastStopIssues ?? []).some(about)) {
    void saveCarriedRecord();
  }
}

/**
 * Note what Staging's write queue did with one conversation. The record
 * changes only when some Staging row is about that conversation, so it is
 * written again only then (#1688).
 */
function recordFileWritten(event: ImportFileWrittenEvent): void {
  const scratch = runScratch();
  scratch.staged.set(event.file, event.status);
  const about = (issue: ImportIssue) =>
    isStagingRowOfConversation(issue) && issue.conversation === event.file;
  if (scratch.issues.some(about) || (scratch.carried.lastStopIssues ?? []).some(about)) {
    void saveCarriedRecord();
  }
}

export function recordError(stage: ImportIssueStage, message: string): void {
  runScratch().issues.push({ kind: "error", stage, item: RUN_ERROR_ITEM, reason: message });
}

/**
 * Run one desktop job to its end, feeding its progress and issues into the
 * run. A job that a Cancel came before is never started, and fails with
 * `CANCELLED_MESSAGE` the way a job cancelled while it runs does.
 */
export function runJob(invokeFn: () => Promise<void>): Promise<DesktopJobResult> {
  const guarded = runScratch().runCancel.guard(invokeFn);
  return awaitDesktopJob(
    "Import Run",
    async () => {
      stopIfAccountLeft();
      await guarded();
    },
    undefined,
    applyProgress,
    recordIssue,
    recordFileDone,
    recordFileWritten,
  );
}

/**
 * `invokeSummarizeStaging`, with a listener on the same `desktop-job:progress`
 * channel Staging and the Media stage use. `summarize_staging` (Rust)
 * emits progress on the `check` step while it walks a big directory, and
 * `applyProgress` already knows to draw that on the Staging row, so this
 * only has to make sure the event reaches it.
 */
export async function summarizeStagingWithProgress(config: RunDirConfig): Promise<StagingSummary> {
  const unlisten = await onDesktopJobEvents({
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
