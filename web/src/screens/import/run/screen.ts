/**
 * What the Import screen shows of the run: its progress rows and the phase
 * it is in. Every stage moves the screen through these.
 */

import { errorText } from "../../../lib/apiErrorMessage";
import type { AttachmentMediaMode } from "../../../lib/types";
import {
  type ImportStep,
  MEDIA_LABEL,
  nextPhase,
  STAGING_LABEL,
  stepsFor,
} from "../importProgressState";
import { CLEARED_RUN } from "../importRunStore";
import type { RunContext } from "./context";

/** Present-tense verb for the Media stage, following the mode so compress mode never says "Converting". */
export function mediaVerb(mode: AttachmentMediaMode): string {
  return mode === "compress" ? "Compressing" : "Converting";
}

/** Sentence shown on the Media row once the Media stage finishes. */
export function mediaDoneDetail(mode: AttachmentMediaMode): string {
  return mode === "compress" ? "Compression complete" : "Conversion complete";
}

/** Stage rows for this mode, with Staging optionally marked active. */
export function initialSteps(
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
 * only when `mediaDone` says the Media stage already finished in an earlier run. A
 * resume at `media` passes `mediaDone: false` and then calls
 * `runMediaStage`, which marks that same row active once it starts.
 */
export function resumeSteps(
  attachmentMedia: AttachmentMediaMode,
  mediaDone: boolean,
): ImportStep[] {
  return stepsFor(attachmentMedia).map((step) => {
    if (step.label === STAGING_LABEL) return { ...step, status: "done", detail: "Already staged" };
    if (step.label === MEDIA_LABEL && mediaDone) {
      return { ...step, status: "done", detail: mediaDoneDetail(attachmentMedia) };
    }
    return step;
  });
}

export function updateSteps(ctx: RunContext, update: (steps: ImportStep[]) => ImportStep[]): void {
  ctx.store.set((state) => ({ steps: update(state.steps) }));
}

/** Mark whichever row is active as failed. */
export function failActiveStep(ctx: RunContext): void {
  updateSteps(ctx, (steps) =>
    steps.map((step) => (step.status === "active" ? { ...step, status: "error" } : step)),
  );
}

export function setRowByLabel(ctx: RunContext, label: string, patch: Partial<ImportStep>): void {
  updateSteps(ctx, (steps) =>
    steps.map((step) => (step.label === label ? { ...step, ...patch } : step)),
  );
}

/**
 * Back to the form. The run's record stays on the server; only what the
 * screen holds goes. `resumeError` is kept on purpose (see the store).
 */
export function returnToForm(ctx: RunContext): void {
  ctx.store.set((state) => ({
    ...CLEARED_RUN,
    phase: nextPhase(state.phase, "back_to_form"),
    form: null,
  }));
}

/**
 * Back to the form with `e` on `resumeError`: reading the run directory
 * failed after its stage had done its work, so the run stays open where it
 * is, and the form's resume check offers it again.
 */
export function returnToFormWithError(ctx: RunContext, e: unknown): void {
  ctx.store.set({
    resumeError: errorText(e),
    computingSummary: false,
    running: false,
  });
  returnToForm(ctx);
}

/** Stop at a review: the run waits, and the review takes the screen. */
export function waitAtReview(ctx: RunContext, review: "staging_review" | "media_review"): void {
  const event = review === "staging_review" ? "stop_at_staging_review" : "stop_at_media_review";
  ctx.store.set((state) => ({
    phase: nextPhase(state.phase, event),
    running: false,
    computingSummary: false,
  }));
}
