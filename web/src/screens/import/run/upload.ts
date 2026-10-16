import type { DesktopJobResult } from "../../../lib/desktop/job";
import { CanceledError } from "../../../lib/desktopJobError";
import { registerRunningUpload } from "../../../lib/runningUpload";
import { sessionRefused } from "../../../lib/sessionRefusal";
import { importOutcome } from "../importOutcome";
import { UPLOAD_LABEL } from "../importProgressState";
import { wholeRun } from "../runRecord";
import type { OpenRun, RunContext } from "./context";
import { jobErrorReason, recordError, runJob } from "./desktopJob";
import { finishImport, stopStageNotRecorded } from "./finish";
import { currentPart } from "./scratch";
import { failActiveStep, movePhase, setRowByLabel } from "./screen";
import { moveStage } from "./serverCalls";

/**
 * Upload to the server and record the outcome: the tail end shared by a
 * resumed run (jumps straight here), the Staging Review when there is no
 * Media Stage, and the Media Review. Never throws: an Upload failure is
 * folded into the finished summary via `finishImport`, exactly like any
 * other terminal outcome.
 */
export async function runUpload(
  ctx: RunContext,
  token: string | null,
  run: OpenRun,
): Promise<void> {
  // Logging out pauses this Upload before it ends the session the Upload
  // sends (`lib/runningUpload.ts`), and waits until the pause is recorded.
  const runCancel = ctx.scratch.runCancel;
  const upload = uploadAndFinish(ctx, token, run);
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
  ctx: RunContext,
  token: string | null,
  run: OpenRun,
): Promise<boolean> {
  const { runId, runDir, approved } = run;
  movePhase(ctx, "upload_start", { running: true });
  ctx.scratch.activeStage = "upload";
  setRowByLabel(ctx, UPLOAD_LABEL, { status: "active", detail: "Uploading to Message Crate…" });
  try {
    await moveStage(ctx, runId, "upload", approved);
  } catch (e: unknown) {
    await stopStageNotRecorded(ctx, e, runId, "upload");
    return false;
  }

  const uploadStartedAt = ctx.now();
  let uploadResult: DesktopJobResult | null = null;
  let threw = false;
  // A Pause that came before the Upload started: the guard refused the job.
  let pausedBeforeStart = false;
  try {
    const baseUrl = ctx.server.baseUrl();
    if (!token) throw new Error("Not authenticated");
    uploadResult = await runJob(ctx, () =>
      ctx.desktop.invokeUpload({
        base_url: baseUrl,
        token,
        input_dir: runDir,
        import_id: runId,
      }),
    );
  } catch (e: unknown) {
    if (e instanceof CanceledError) {
      // The person asked for this: not an error, so no issue row for it.
      pausedBeforeStart = true;
    } else {
      threw = true;
      recordError(ctx, ctx.scratch.activeStage, jobErrorReason(e));
      failActiveStep(ctx);
    }
  }
  const uploadMs = ctx.now() - uploadStartedAt;
  const report = uploadResult?.report ?? null;
  // An Upload that did not send every conversation, paused or failed, is
  // paused: the run stays at `upload` with its directory, and resuming it
  // sends only what the Upload journal does not list.
  const status = pausedBeforeStart
    ? "paused"
    : importOutcome({
        report: report ?? undefined,
        threw,
        issues: wholeRun(ctx.scratch.carried, currentPart(ctx, report, uploadMs)).issues,
        approved,
      });
  if (status === "paused") {
    setRowByLabel(ctx, UPLOAD_LABEL, { status: "error", detail: "Paused", durationMs: uploadMs });
  } else {
    setRowByLabel(ctx, UPLOAD_LABEL, {
      status: "done",
      detail: "Upload complete",
      durationMs: uploadMs,
    });
  }

  await finishImport(ctx, { runId, status, uploadReport: report, uploadMs });
  return report?.session_refused === true;
}
