import { errorLine } from "../../../lib/apiErrorMessage";
import { CanceledError } from "../../../lib/canceledError";
import type { ImportJobFormValues } from "../../../lib/importSources/types";
import {
  ffmpegMissing,
  type MediaToolName,
  type TranscodeFinishedReport,
} from "../../../lib/tauri";
import type { AttachmentMediaMode } from "../../../lib/types";
import { MEDIA_LABEL } from "../importProgressState";
import { mediaJobVerb } from "../reviewForecast";
import type { OpenRun, RunContext } from "./context";
import { recordError, runJob, summarizeStagingWithProgress } from "./desktopJob";
import { finishImport, stopStageNotRecorded } from "./finish";
import { saveCarriedRecord } from "./runRecordWrites";
import {
  failActiveStep,
  mediaDoneDetail,
  mediaVerb,
  movePhase,
  returnToFormWithError,
  setRowByLabel,
  waitAtReview,
} from "./screen";
import { moveStage, moveStageAtReview } from "./serverCalls";

/**
 * Which of ffmpeg and ffprobe this mode needs and cannot use; empty when it
 * needs neither or both are found. Both when the desktop process can't say.
 */
export async function mediaToolsMissingFor(
  ctx: RunContext,
  mode: AttachmentMediaMode,
): Promise<MediaToolName[]> {
  if (mediaJobVerb(mode) === null) return [];
  try {
    return ffmpegMissing(await ctx.desktop.invokeToolsStatus());
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
 */
export async function runMediaStage(
  ctx: RunContext,
  form: ImportJobFormValues,
  run: OpenRun,
): Promise<void> {
  const { store } = ctx;
  const { runId, runDir, approved } = run;
  movePhase(ctx, "media_start", { running: true });
  ctx.scratch.activeStage = "media";
  setRowByLabel(ctx, MEDIA_LABEL, {
    status: "active",
    detail: `${mediaVerb(form.attachmentMedia)}…`,
  });

  // Carries the plan approved at the Staging Review even on this stage: a
  // crash mid-stage must not leave `summary_json` null with no baseline for
  // a later resume to diff against.
  try {
    await moveStage(ctx, runId, "media", approved);
  } catch (e: unknown) {
    await stopStageNotRecorded(ctx, e, runId, "media");
    return;
  }

  const mediaStartedAt = ctx.now();
  let transcodeReport: TranscodeFinishedReport | undefined;
  let threw = false;
  let cancelled = false;
  try {
    const result = await runJob(ctx, () => ctx.desktop.invokeTranscodeStaging({ run_dir: runDir }));
    transcodeReport = result.transcode;
  } catch (e: unknown) {
    if (e instanceof CanceledError) {
      // The person asked for this: not an error, so no issue row for it.
      cancelled = true;
    } else {
      threw = true;
      recordError(ctx, ctx.scratch.activeStage, errorLine(e));
    }
  }
  const mediaMs = ctx.now() - mediaStartedAt;

  if (threw || cancelled) {
    failActiveStep(ctx);
    // Neither path writes another stage: the run stays at `media`,
    // which is exactly where it got to. A cancellation also skips
    // `/complete` outright (see finishImport), so the run stays running and
    // resumable instead of completing and freeing the slot out from under a
    // run directory nobody can reach any more. A failed stage is discarded:
    // it completes as failed and its directory goes, so a broken ffmpeg does
    // not lock the account out of importing.
    await finishImport(ctx, {
      runId,
      status: cancelled ? "cancelled" : "failed",
      uploadReport: null,
      uploadMs: null,
      skipComplete: cancelled,
    });
    return;
  }

  setRowByLabel(ctx, MEDIA_LABEL, {
    status: "done",
    detail: mediaDoneDetail(form.attachmentMedia),
    durationMs: mediaMs,
  });

  store.set({ computingSummary: true });
  if ((await moveStageAtReview(ctx, runId, "media_review", approved)) === "refused") {
    return;
  }
  // Media's times are only in memory until now, and the run may be resumed
  // from this Review after the app closes.
  await saveCarriedRecord(ctx);
  try {
    const actual = await summarizeStagingWithProgress(ctx, { run_dir: runDir });
    store.set({ mediaSummary: actual, mediaFailedCount: transcodeReport?.failed ?? null });
    waitAtReview(ctx, "media_review");
  } catch (e: unknown) {
    // Media itself succeeded; only reading the directory afterwards failed.
    // The converted directory is the run's work, so the run is not completed
    // and its directory stays: back to the form, as after Staging, with the
    // failure on `resumeError`. The run waits at the Media Review on the
    // server, and resuming it there reads the directory again.
    returnToFormWithError(ctx, e);
  }
}
