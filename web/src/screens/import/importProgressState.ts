import type { ImportIssue, ImportNote } from "../../components/import/ImportSummaryPanel";
import type { ImportIssueStage } from "../../components/import/importIssueStage";
import { formatAttachmentProgress } from "../../lib/attachmentProgressCopy";
import type { AttachmentMediaMode, ImportIssueEvent, ImportProgressEvent } from "../../lib/types";

export type ImportStep = {
  label: string;
  status: "pending" | "active" | "done" | "error";
  detail?: string;
  durationMs?: number | null;
};

/**
 * Where the Import screen is. The three stages of a run (CONTEXT.md,
 * "Stage") all show as `running`; the two reviews are phases of their
 * own, because the run is waiting for the person rather than working.
 * Only `nextPhase` decides which phase follows which. The one write outside
 * it is the run service's `reset`, which replaces the whole store with a
 * fresh form rather than moving a run on.
 */
export const IMPORT_PHASES = [
  "form",
  "identity_stop",
  "running",
  "staging_review",
  "media_review",
  "done",
] as const;
export type ImportPhase = (typeof IMPORT_PHASES)[number];

/**
 * What moves the Import screen to another phase: one event for each kind of
 * phase write in the run's modules (`run/`).
 */
export const IMPORT_PHASE_EVENTS = [
  /** The backup sent from no address of the account's: ask the person first. */
  "identity_stop",
  /** Staging starts: a new run, a continue past the identity stop, or a resumed Staging or Upload. */
  "staging_start",
  /** A run waiting at a review is resumed: its directory is read again before the review shows. */
  "resume",
  /** Staging ended, or a resume read the directory: wait at the Staging Review. */
  "stop_at_staging_review",
  /** Media ended, or a resume read the directory: wait at the Media Review. */
  "stop_at_media_review",
  /** Media starts: approved at the Staging Review, or a run resumed mid Media. */
  "media_start",
  /** Upload starts: approved at a review, or a resumed Upload once Staging's start ran. */
  "upload_start",
  /** The run ended: finished, failed, paused or cancelled. */
  "finish",
  /** Back to the form, from wherever the screen is. */
  "back_to_form",
] as const;
export type ImportPhaseEvent = (typeof IMPORT_PHASE_EVENTS)[number];

/** For each event, the phases it may come in and the phase it moves to. */
const PHASE_MOVES: Record<ImportPhaseEvent, { from: readonly ImportPhase[]; to: ImportPhase }> = {
  identity_stop: { from: ["form"], to: "identity_stop" },
  staging_start: { from: ["form", "identity_stop"], to: "running" },
  resume: { from: ["form"], to: "running" },
  // Not only after Staging: a run the server has mid Media, resumed with
  // ffmpeg missing, waits here too, a phase before the stage it stopped in.
  stop_at_staging_review: { from: ["running"], to: "staging_review" },
  stop_at_media_review: { from: ["running"], to: "media_review" },
  media_start: { from: ["staging_review", "form"], to: "running" },
  upload_start: { from: ["staging_review", "media_review", "running"], to: "running" },
  finish: { from: ["running"], to: "done" },
  back_to_form: { from: IMPORT_PHASES, to: "form" },
};

/**
 * The phase the Import screen moves to from `phase` on `event`: the one place
 * that decides it, as `local_server.rs`'s `step` is for the desktop's server.
 * Every phase write in `run/` goes through it, by way of `movePhase`.
 *
 * Unlike `step`, which leaves its state alone on an event it has no rule for,
 * a move the table does not hold throws, so the test that runs the write
 * fails. It is a bug in that write. In the app, none of that write lands, and
 * what the person sees depends on the caller. Where the caller catches the
 * error, as the review stops after a directory read do, the screen goes back
 * to the form with the error's text, as if the read had failed. Elsewhere the
 * action stops there, the screen stays in the phase it was in, and the error
 * reaches the console.
 */
export function nextPhase(phase: ImportPhase, event: ImportPhaseEvent): ImportPhase {
  const move = PHASE_MOVES[event];
  if (!move.from.includes(phase)) {
    throw new Error(`Import phase: no move from ${phase} on ${event}`);
  }
  return move.to;
}

export type AttachmentProgressCounts = {
  done: number;
  total: number;
  bytesDone: number;
  bytesTotal: number;
};

/** The three stages of an Import Run, by the names the glossary gives them. */
export const STAGING_LABEL = "Staging";
export const MEDIA_LABEL = "Media";
export const UPLOAD_LABEL = "Upload";

/** Whether this attachment media mode runs a Media stage (convert/compress). */
export function hasMediaStage(mode: AttachmentMediaMode): boolean {
  return mode === "convert" || mode === "compress";
}

/**
 * The stage rows for this attachment media mode: three under convert and
 * compress, two under copy and skip. There is no Media stage in copy/skip,
 * so a greyed-out row would be promising work that will never run.
 */
export function stepsFor(mode: AttachmentMediaMode): ImportStep[] {
  const labels = [STAGING_LABEL, ...(hasMediaStage(mode) ? [MEDIA_LABEL] : []), UPLOAD_LABEL];
  return labels.map((label) => ({ label, status: "pending" }));
}

/**
 * Row index for every step this build knows, keyed by the step name — a
 * `Record` over `ImportProgressEvent["step"]` so a step added to that union
 * without a row here is a compile error, not a silent runtime fallback.
 *
 * Reading the backup, copying its attachments and writing the conversation
 * files are all Staging from the person's side, so the first five steps
 * narrate the one Staging row rather than rows of their own.
 */
const STEP_ROW_INDEX: Record<ImportProgressEvent["step"], (mode: AttachmentMediaMode) => number> = {
  setup: () => 0,
  parse: () => 0,
  attachments: () => 0,
  prepare: () => 0,
  check: () => 0,
  media: (mode) => (hasMediaStage(mode) ? 1 : -1),
  upload: (mode) => (hasMediaStage(mode) ? 2 : 1),
};

/**
 * Index of the stage row that matches this event, in this mode's row list.
 * Returns -1 for a step with no row in this mode (`media` under copy/skip),
 * or for a step string this build does not recognise at all — the event
 * comes off the wire unvalidated, so a lookup miss must resolve to "no row",
 * never `undefined`. Callers must treat -1 as "no row to update", not index
 * with it.
 */
export function stepIndexFor(step: ImportProgressEvent["step"], mode: AttachmentMediaMode): number {
  return STEP_ROW_INDEX[step]?.(mode) ?? -1;
}

/**
 * The Stage (CONTEXT.md) a desktop progress step belongs to. Setting up,
 * reading the backup, copying its attachments and writing the conversation
 * files are all Staging.
 */
export function stageForStep(step: ImportProgressEvent["step"]): ImportIssueStage {
  if (step === "media") return "media";
  if (step === "upload") return "upload";
  return "staging";
}

/**
 * An issue the desktop app reported, as the run records it. The server takes
 * it without `conversation` (`issueRequests`).
 */
export function issueFromEvent(event: ImportIssueEvent): ImportIssue {
  const issue: ImportIssue = {
    kind: event.kind,
    stage: stageForStep(event.step),
    item: event.item,
    reason: event.reason,
  };
  if (event.conversation != null) issue.conversation = event.conversation;
  return issue;
}

/** A note the desktop app reported, as the run records it. */
export function noteFromEvent(event: ImportIssueEvent): ImportNote {
  return { stage: stageForStep(event.step), item: event.item, text: event.reason };
}

/**
 * Whether a progress event should mark its row done. Attachments stay active
 * until prepare, and a setup event never completes the Staging row: its
 * counts are "step 5 of 5", and the messages are still to be read once that
 * lands. Parse does not complete Staging either: the attachments and the
 * conversation files still follow it on the same row.
 */
export function isProgressStepComplete(
  step: ImportProgressEvent["step"],
  done: number,
  total: number,
): boolean {
  if (step === "attachments" || step === "setup" || step === "parse") return false;
  return total > 0 && done >= total;
}

/**
 * Detail line for a setup event: the step's label with its position, so a
 * long decrypt reads as "Deriving backup keys (1/5)" rather than a frozen
 * "Reading backup…".
 */
export function setupDetail(event: Pick<ImportProgressEvent, "done" | "total" | "status">): string {
  const label = event.status ?? "Preparing";
  return event.total > 0 ? `${label} (${event.done}/${event.total})` : label;
}

/** Done-line for the attachment copy, using the last live counts when present. */
export function attachmentDoneDetail(
  mode: AttachmentMediaMode,
  counts: AttachmentProgressCounts | null,
): string {
  return formatAttachmentProgress({
    mode,
    done: counts?.done ?? 0,
    total: counts?.total ?? 0,
    bytesDone: counts?.bytesDone ?? 0,
    bytesTotal: counts?.bytesTotal ?? 0,
  });
}

/**
 * When each Staging stage first and last reported, from `performance.now()`.
 * The stages overlap: messages are still being read while attachments copy,
 * and conversation files are written while attachments copy, so no stage's
 * event marks the end of another.
 */
export type StageTiming = {
  extractStartedAt: number | null;
  parseStartedAt: number | null;
  parseEndedAt: number | null;
  attachmentsStartedAt: number | null;
  attachmentsEndedAt: number | null;
  prepareStartedAt: number | null;
  prepareEndedAt: number | null;
};

export const EMPTY_TIMING: StageTiming = {
  extractStartedAt: null,
  parseStartedAt: null,
  parseEndedAt: null,
  attachmentsStartedAt: null,
  attachmentsEndedAt: null,
  prepareStartedAt: null,
  prepareEndedAt: null,
};

/**
 * Timing after one progress event at `now`: the event's stage starts at its
 * first event and ends at its latest. The progress sink always delivers a
 * stage's last count, so the latest event is when the stage finished.
 * Decrypting and caching (`setup`) are the start of reading the backup, so
 * they start the read timer without ending it.
 */
export function recordStageTime(
  timing: StageTiming,
  step: ImportProgressEvent["step"],
  now: number,
): StageTiming {
  switch (step) {
    case "setup":
      return { ...timing, parseStartedAt: timing.parseStartedAt ?? now };
    case "parse":
      return { ...timing, parseStartedAt: timing.parseStartedAt ?? now, parseEndedAt: now };
    case "attachments":
      return {
        ...timing,
        attachmentsStartedAt: timing.attachmentsStartedAt ?? now,
        attachmentsEndedAt: now,
      };
    case "prepare":
      return { ...timing, prepareStartedAt: timing.prepareStartedAt ?? now, prepareEndedAt: now };
    default:
      return timing;
  }
}

/**
 * Read, attachment, and prepare durations once extract has finished. A stage
 * that never reported took no time. Reading with no message count (only
 * setup events) runs until the extract finished.
 */
export function stageDurations(
  timing: StageTiming,
  extractFinishedAt: number,
): { parseMs: number; attachmentsMs: number; prepareMs: number } {
  const span = (start: number | null, end: number | null) =>
    start == null ? 0 : Math.max(0, (end ?? extractFinishedAt) - start);
  return {
    parseMs: span(timing.parseStartedAt ?? timing.extractStartedAt, timing.parseEndedAt),
    attachmentsMs: span(timing.attachmentsStartedAt, timing.attachmentsEndedAt),
    prepareMs: span(timing.prepareStartedAt, timing.prepareEndedAt),
  };
}
