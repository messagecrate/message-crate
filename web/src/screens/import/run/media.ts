import { errorText } from "../../../lib/apiErrorMessage";
import type { ImportJobFormValues } from "../../../lib/importSources/types";
import { CANCELLED_MESSAGE } from "../../../lib/runCancel";
import {
  ffmpegMissing,
  invokeToolsStatus,
  invokeTranscodeStaging,
  type MediaToolName,
  type StagingSummary,
  type TranscodeFinishedReport,
} from "../../../lib/tauri";
import type { AttachmentMediaMode } from "../../../lib/types";
import { MEDIA_LABEL } from "../importProgressState";
import { importRunStore as store } from "../importRunStore";
import { mediaJobVerb } from "../reviewForecast";
import { recordError, runJob, summarizeStagingWithProgress } from "./desktopJob";
import { finishImport } from "./finish";
import { saveCarriedRecord } from "./runRecordWrites";
import { runScratch } from "./scratch";
import { leaveIfRefused, moveStage, moveStageAtReview } from "./serverCalls";
import {
  failActiveStep,
  mediaDoneDetail,
  mediaVerb,
  returnToForm,
  setRowByLabel,
  waitAtReview,
} from "./steps";

/**
 * Which of ffmpeg and ffprobe this mode needs and cannot use; empty when it
 * needs neither or both are found. Both when the desktop process can't say.
 */
export async function mediaToolsMissingFor(mode: AttachmentMediaMode): Promise<MediaToolName[]> {
  if (mediaJobVerb(mode) === null) return [];
  try {
    return ffmpegMissing(await invokeToolsStatus());
  } catch {
    return ["ffmpeg", "ffprobe"];
  }
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
export async function runMediaStage(
  form: ImportJobFormValues,
  runId: number,
  outputDir: string,
  approvedSummary?: StagingSummary,
): Promise<void> {
  store.set({ running: true, phase: "running" });
  runScratch().activeStage = "media";
  setRowByLabel(MEDIA_LABEL, { status: "active", detail: `${mediaVerb(form.attachmentMedia)}…` });

  // Carries the plan approved at the Staging Review even on this stage: a
  // crash mid-stage must not leave `summary_json` null with no baseline for
  // a later resume to diff against.
  try {
    await moveStage(runId, "media", approvedSummary);
  } catch (e: unknown) {
    if (await leaveIfRefused(e, runId)) return;
    // The server still has the run at the Staging Review, so the run stays
    // there and is not completed: a later visit offers that review again.
    recordError("media", errorText(e));
    failActiveStep();
    await finishImport({
      runId,
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
    const result = await runJob(() => invokeTranscodeStaging({ run_dir: outputDir }));
    transcodeReport = result.transcode;
  } catch (e: unknown) {
    const msg = errorText(e);
    if (msg === CANCELLED_MESSAGE) {
      // The person asked for this: not an error, so no issue row for it.
      cancelled = true;
    } else {
      threw = true;
      recordError(runScratch().activeStage, msg);
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
      runId,
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
  if ((await moveStageAtReview(runId, "media_review", approvedSummary)) === "refused") return;
  // Media's times are only in memory until now, and the run may be resumed
  // from this Review after the app closes.
  await saveCarriedRecord();
  try {
    const actual = await summarizeStagingWithProgress({ run_dir: outputDir });
    store.set({ mediaSummary: actual, mediaFailedCount: transcodeReport?.failed ?? null });
    waitAtReview("media_review");
  } catch (e: unknown) {
    // Media itself succeeded; only reading the directory afterwards failed.
    // The converted directory is the run's work, so the run is not completed
    // and its directory stays: back to the form, as after Staging, with the
    // failure on `resumeError`. The run waits at the Media Review on the
    // server, and resuming it there reads the directory again.
    store.set({
      resumeError: errorText(e),
      computingSummary: false,
      running: false,
    });
    returnToForm();
  }
}
