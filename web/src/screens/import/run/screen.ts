/**
 * What the Import screen shows of the run: its progress rows and the phase
 * it is in. Every stage moves the screen through these.
 */

import { errorText } from "../../../lib/apiErrorMessage";
import type { AttachmentMediaMode } from "../../../lib/types";
import { type ImportStep, MEDIA_LABEL, STAGING_LABEL, stepsFor } from "../importProgressState";
import { CLEARED_RUN, importRunStore as store } from "../importRunStore";

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

export function updateSteps(update: (steps: ImportStep[]) => ImportStep[]): void {
  store.set((state) => ({ steps: update(state.steps) }));
}

/** Mark whichever row is active as failed. */
export function failActiveStep(): void {
  updateSteps((steps) =>
    steps.map((step) => (step.status === "active" ? { ...step, status: "error" } : step)),
  );
}

export function setRowByLabel(label: string, patch: Partial<ImportStep>): void {
  updateSteps((steps) =>
    steps.map((step) => (step.label === label ? { ...step, ...patch } : step)),
  );
}

/**
 * Back to the form. The run's record stays on the server; only what the
 * screen holds goes. `resumeError` is kept on purpose (see the store).
 */
export function returnToForm(): void {
  store.set({
    ...CLEARED_RUN,
    phase: "form",
    form: null,
  });
}

/**
 * Back to the form with `e` on `resumeError`: reading the run directory
 * failed after its stage had done its work, so the run stays open where it
 * is, and the form's resume check offers it again.
 */
export function returnToFormWithError(e: unknown): void {
  store.set({
    resumeError: errorText(e),
    computingSummary: false,
    running: false,
  });
  returnToForm();
}

/** Stop at a review: the run waits, and the review takes the screen. */
export function waitAtReview(phase: "staging_review" | "media_review"): void {
  store.set({ phase, running: false, computingSummary: false });
}
