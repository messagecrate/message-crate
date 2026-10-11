import { getAccountId, getBaseUrl } from "../../../lib/api";
import { errorText } from "../../../lib/apiErrorMessage";
import { getDeviceId } from "../../../lib/deviceId";
import { buildSourceFingerprint } from "../../../lib/importRun";
import { importRunCreateBody } from "../../../lib/importSource";
import { importSourceFor } from "../../../lib/importSources";
import type { ImportJobFormValues } from "../../../lib/importSources/types";
import { CANCELLED_MESSAGE } from "../../../lib/runCancel";
import { createImport, getServerState } from "../../../lib/serverApi";
import {
  invokeCreateRunDir,
  invokeExtract,
  invokePathStat,
  invokeStartImportRunLog,
  type StagingSummary,
} from "../../../lib/tauri";
import { isTauri } from "../../../lib/tauri-check";
import { formSnapshot } from "../formSnapshot";
import {
  attachmentDoneDetail,
  STAGING_LABEL,
  stageDurations,
  stepsFor,
  UPLOAD_LABEL,
} from "../importProgressState";
import { CLEARED_RUN, importRunStore as store } from "../importRunStore";
import { recordError, runJob, summarizeStagingWithProgress } from "./desktopJob";
import { finishImport } from "./finish";
import { mediaToolsMissingFor } from "./media";
import { saveCarriedRecord } from "./runRecordWrites";
import { beginRun, loadCarriedRecord, runScratch } from "./scratch";
import {
  failActiveStep,
  initialSteps,
  returnToFormWithError,
  setRowByLabel,
  updateSteps,
  waitAtReview,
} from "./screen";
import {
  endSessionIfRefused,
  leaveIfRefused,
  moveStage,
  moveStageAtReview,
  StageNotRecordedError,
  stopIfAccountLeft,
} from "./serverCalls";
import { adoptRecordedMode } from "./stagingSummary";
import { runUpload } from "./upload";

/** An Import Run whose copy was interrupted, and the directory it was writing into. */
export type ResumeWrite = {
  runId: number;
  runDir: string;
  /** The list recorded on the run at creation (`source_identities`,
   * parsed by the caller). A resumed write lands back on the Staging Review
   * without re-probing the backup, so this is the only way that Review's identity
   * section gets a list to show. */
  identities?: string[] | null;
};

/** Pick up an Import Run whose run directory is already complete. */
export type ResumeUpload = {
  runId: number;
  runDir: string;
  /** The plan approved at the last Review the run left, parsed from
   * its stored `summary` (`parseStoredStagingSummary`). Undefined when the
   * run recorded nothing usable — `runUpload` and `importOutcome`
   * tolerate that absence, they just can't diff a resumed Upload's expected
   * omissions against it, which demotes an honest `completed` outcome to
   * `completed_with_issues` for exactly the interrupted-and-resumed case. */
  approved?: StagingSummary;
};

/**
 * The form an Import Run is started with, its attachment mode as the form
 * showed it. iMazing and OpenExtract show no Attachments field, so the
 * field may still hold what was chosen for another source; their runs copy
 * attachments, and the run's stored form says so.
 */
function withShownAttachmentMode(form: ImportJobFormValues): ImportJobFormValues {
  return importSourceFor(form.source).showsAttachmentOptions
    ? form
    : { ...form, attachmentMedia: "copy" };
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

/**
 * Start the new run's log in the Logs Directory with the line naming the
 * account that runs it and the Message Crate, so the Logs panel shows the log
 * to that account and the owner only (#1665). A log that cannot be started
 * leaves the run going: its lines still reach the window, and the log is the
 * owner's alone.
 */
async function startRunLog(
  runDir: string,
  importRunId: number,
  messageCrateId: string,
): Promise<void> {
  const accountId = getAccountId();
  if (accountId === null) {
    console.warn("The Import Run's log names no account: no account is signed in.");
    return;
  }
  try {
    await invokeStartImportRunLog(runDir, {
      importRunId,
      accountId,
      server: getBaseUrl(),
      messageCrateId,
    });
  } catch (e) {
    console.warn("The Import Run's log could not be started:", e);
  }
}

export async function runImport(
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
    ...CLEARED_RUN,
    running: true,
    phase: "running",
    form,
    resumeError: null,
  });

  let runId: number | null = null;
  // The id of the Message Crate a new run imports into, for its log's first
  // line. Read with the size limit below, so a new run asks the server once.
  let messageCrateId = "";

  try {
    if (!token) throw new Error("Not authenticated");

    if (!resume && !resumeWrite) {
      // A new Import Run works to the server's attachment size limit as it is
      // now. It goes into the form the run is created with, so every later
      // stage, and a resume, measures against this same number.
      const server = await endSessionIfRefused(() => getServerState());
      messageCrateId = server.id;
      form = { ...form, assetMaxBytes: server.asset_max_bytes };
      runScratch().form = form;
      store.set({ form });
    }

    if (resume) {
      // The run directory is already complete, so there is nothing to
      // resolve, no new run to create (the account already has this one),
      // and no extract to run. resume_upload is only ever offered after the
      // last review, so there IS a plan from it: it rides along as
      // `resume.approved` (parsed from the run's stored summary) when it
      // parses. Straight to Upload.
      const outputDir = resume.runDir;
      runId = resume.runId;
      await loadCarriedRecord(outputDir);
      // The approved plan was read from the directory at its review, so it
      // carries the mode Staging recorded there.
      form = adoptRecordedMode(form, resume.approved);
      store.set({
        runDir: outputDir,
        importRunId: runId,
        steps: stepsFor(form.attachmentMedia).map((step) =>
          step.label === UPLOAD_LABEL
            ? { ...step, status: "active", detail: "Uploading to Message Crate…" }
            : { ...step, status: "done", detail: "Already staged" },
        ),
      });
      await runUpload(token, runId, outputDir, resume.approved);
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
      outputDir = resumeWrite.runDir;
      runId = resumeWrite.runId;
      await loadCarriedRecord(outputDir);
      store.set({ runDir: outputDir, importRunId: runId });
      setRowByLabel(STAGING_LABEL, { detail: "Extracting…" });
      await moveStage(runId, "write");
    } else {
      outputDir = await invokeCreateRunDir(form.source);
      store.set({ runDir: outputDir });

      const backupStat = await invokePathStat(form.backupPath).catch(() => null);
      // The run is created in the account of the session logged in now.
      stopIfAccountLeft();
      const importRun = await endSessionIfRefused(() =>
        createImport({
          ...importRunCreateBody(form.source),
          phone_country: form.phoneCountry || null,
          stage: "parse",
          run_dir: outputDir,
          device_id: getDeviceId(),
          form: formSnapshot(form),
          source_fingerprint: backupStat
            ? buildSourceFingerprint(form.backupPath, backupStat)
            : null,
          source_identities: identities,
        }),
      );
      runId = importRun.id;
      store.set({ importRunId: runId });
      await startRunLog(outputDir, runId, messageCrateId);
      setRowByLabel(STAGING_LABEL, { detail: "Extracting…" });
      await moveStage(runId, "write");
    }

    runScratch().timing.extractStartedAt = performance.now();
    const extractResult = await runJob(() =>
      invokeExtract({
        source: form.source,
        path: form.backupPath,
        output_dir: outputDir,
        ...(resumeWrite ? { resume: true } : {}),
        asset_max_bytes: assetLimitOf(form),
        // The media fields go only to a source whose form shows them, as the
        // person chose them: extract checks them before anything is staged
        // and records them for the later stages.
        ...importSourceFor(form.source).extractFields(form),
        ...(form.phoneCountry ? { phone_country: form.phoneCountry } : {}),
      }),
    );
    const scratch = runScratch();
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

    if ((await moveStageAtReview(runId, "staging_review")) === "refused") return;
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
      const summary = await summarizeStagingWithProgress({ run_dir: outputDir });
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
      returnToFormWithError(e);
    }
  } catch (e: unknown) {
    if (await leaveIfRefused(e, runId)) return;
    const msg = errorText(e);
    // A cancelled Staging is not a failure: the conversations already
    // written are real work, and Staging can pick up from them. Leaving the
    // run at `write` is what lets the next Import visit offer that. A
    // `write` stage the server did not record stops the run where the server
    // has it, so it is not completed either. A genuine failure still
    // completes: a broken backup must not lock the account out of importing.
    const cancelled = msg === CANCELLED_MESSAGE;
    const stageNotRecorded = e instanceof StageNotRecordedError;
    if (!cancelled) recordError(runScratch().activeStage, msg);
    failActiveStep();
    store.set({ computingSummary: false });
    await finishImport({
      runId,
      status: cancelled ? "cancelled" : "failed",
      uploadReport: null,
      uploadMs: null,
      skipComplete: cancelled || stageNotRecorded,
    });
  }
}
