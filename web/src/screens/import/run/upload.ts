import { getBaseUrl } from "../../../lib/api";
import { errorText } from "../../../lib/apiErrorMessage";
import { CANCELLED_MESSAGE } from "../../../lib/runCancel";
import { registerRunningUpload } from "../../../lib/runningUpload";
import { sessionRefused } from "../../../lib/sessionRefusal";
import { type DesktopJobResult, invokeUpload, type StagingSummary } from "../../../lib/tauri";
import { importOutcome } from "../importOutcome";
import { UPLOAD_LABEL } from "../importProgressState";
import { importRunStore as store } from "../importRunStore";
import { wholeRun } from "../runRecord";
import { recordError, runJob } from "./desktopJob";
import { finishImport } from "./finish";
import { currentPart, runScratch } from "./scratch";
import { leaveIfRefused, moveStage } from "./serverCalls";
import { failActiveStep, setRowByLabel } from "./steps";

/**
 * Upload to the server and record the outcome: the tail end shared by a
 * resumed run (jumps straight here), the Staging Review when there is no
 * Media stage, and the Media Review. Never throws: an Upload failure is
 * folded into the finished summary via `finishImport`, exactly like any
 * other terminal outcome.
 */
export async function runUpload(
  token: string | null,
  runId: number,
  outputDir: string,
  approvedPlan?: StagingSummary,
): Promise<void> {
  // Logging out pauses this Upload before it ends the session the Upload
  // sends (`lib/runningUpload.ts`), and waits until the pause is recorded.
  const runCancel = runScratch().runCancel;
  const upload = uploadAndFinish(token, runId, outputDir, approvedPlan);
  const pause = async () => {
    await runCancel.cancel();
    await upload;
  };
  const ended = registerRunningUpload(pause);
  let pushRefused = false;
  try {
    pushRefused = await upload;
  } finally {
    ended();
  }
  // The Upload paused because the server refused its session: every request
  // with that token is refused now, so the session ends here too (#1491).
  if (pushRefused && token) sessionRefused(token);
}

/**
 * `runUpload` without the registration that lets logging out pause it.
 * Resolves to whether the server refused the session the Upload sent.
 */
async function uploadAndFinish(
  token: string | null,
  runId: number,
  outputDir: string,
  approvedPlan?: StagingSummary,
): Promise<boolean> {
  store.set({ running: true, phase: "running" });
  runScratch().activeStage = "upload";
  setRowByLabel(UPLOAD_LABEL, { status: "active", detail: "Uploading to Message Crate…" });
  try {
    await moveStage(runId, "upload", approvedPlan);
  } catch (e: unknown) {
    if (await leaveIfRefused(e, runId)) return false;
    // The server still has the run at its review, so the run stays there
    // and is not completed: a later visit offers that review again.
    recordError("upload", errorText(e));
    failActiveStep();
    await finishImport({
      runId,
      status: "failed",
      uploadReport: null,
      uploadMs: null,
      skipComplete: true,
    });
    return false;
  }

  const uploadStartedAt = performance.now();
  let uploadResult: DesktopJobResult | null = null;
  let threw = false;
  // A Pause that came before the Upload started: the guard refused the job.
  let pausedBeforeStart = false;
  try {
    const baseUrl = getBaseUrl();
    if (!token) throw new Error("Not authenticated");
    uploadResult = await runJob(() =>
      invokeUpload({
        base_url: baseUrl,
        token,
        input_dir: outputDir,
        mode: "append",
        import_id: runId,
      }),
    );
  } catch (e: unknown) {
    const msg = errorText(e);
    if (msg === CANCELLED_MESSAGE) {
      // The person asked for this: not an error, so no issue row for it.
      pausedBeforeStart = true;
    } else {
      threw = true;
      recordError(runScratch().activeStage, msg);
      failActiveStep();
    }
  }
  const uploadMs = performance.now() - uploadStartedAt;
  const report = uploadResult?.report ?? null;
  // An Upload that did not send every conversation, paused or failed, is
  // paused: the run stays at `upload` with its directory, and resuming it
  // sends only what the Upload journal does not list.
  const status = pausedBeforeStart
    ? "paused"
    : importOutcome({
        report: report ?? undefined,
        threw,
        issues: wholeRun(runScratch().carried, currentPart(report, uploadMs)).issues,
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

  await finishImport({ runId, status, uploadReport: report, uploadMs });
  return report?.session_refused === true;
}
