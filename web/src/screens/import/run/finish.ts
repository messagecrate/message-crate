import type {
  ImportIssue,
  ImportNote,
  ImportSummaryView,
} from "../../../components/import/ImportSummaryPanel";
import { errorText } from "../../../lib/apiErrorMessage";
import { discardImportRun } from "../../../lib/importRun";
import { completeImport } from "../../../lib/serverApi";
import { invokeReadImportRunRecord, type UploadFinishedReport } from "../../../lib/tauri";
import { STAGING_LABEL, UPLOAD_LABEL } from "../importProgressState";
import { importRunStore as store } from "../importRunStore";
import {
  filesSkippedOverRun,
  issueRequests,
  issuesToDiscard,
  notesToDiscard,
  parseRunRecord,
  RUN_ERROR_ITEM,
  wholeRun,
} from "../runRecord";
import { discardRunDirectory } from "./runDirectory";
import { recordWritesSettled, saveCarriedRecord } from "./runRecordWrites";
import { currentPart, runScratch } from "./scratch";
import { endSessionIfRefused, SessionRefusedError } from "./serverCalls";
import { setRowByLabel, updateSteps } from "./steps";

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
export async function finishImport(args: {
  runId: number | null;
  status: "completed" | "completed_with_issues" | "failed" | "cancelled" | "paused";
  uploadReport: UploadFinishedReport | null;
  uploadMs: number | null;
  skipComplete?: boolean;
}): Promise<void> {
  const { runId, status, uploadReport, uploadMs, skipComplete } = args;
  const carried = runScratch().carried;
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
  const posts = runId != null && !skipComplete && status !== "paused";
  let completeRefused: string | null = null;
  let sessionWasRefused = false;
  if (posts) {
    try {
      // The server counts the messages and attachments the run holds: a
      // resumed Upload's report counts only what the resume sent.
      await endSessionIfRefused(() =>
        completeImport(runId, {
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
        }),
      );
    } catch (e: unknown) {
      completeRefused = errorText(e);
      sessionWasRefused = e instanceof SessionRefusedError;
    }
  }
  if (completeRefused != null) {
    if (status === "completed" || status === "completed_with_issues") {
      finalSummary.status = "paused";
      setRowByLabel(UPLOAD_LABEL, { status: "error", detail: "Paused" });
    }
    // Shown here only, since the server never took the issues it would be
    // recorded with. A refused session is no fault of the run: the session
    // has ended, and the next login completes the run.
    if (!sessionWasRefused) {
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
  }
  // A run with no server record at all (its creation failed) is ended too:
  // nothing will ever offer its directory again.
  const runEnded = runId == null || (posts && completeRefused == null);
  let runDir = store.get().runDir;
  if (runEnded) {
    // An ended run's directory goes: the staged messages, the Upload's log, journal
    // and report, and the run record. When the delete fails, the directory link
    // stays and the failure is shown, so the person can find what was left
    // and remove it by hand. A record write still on its way finishes
    // first, so it cannot land in the directory after the delete.
    await recordWritesSettled();
    if (runDir != null && (await discardRunDirectory(runDir))) runDir = null;
  } else {
    await saveCarriedRecord(uploadReport, uploadMs);
  }
  // The server writes this run's saved search and Contact Group when the run
  // completes, so a window closed mid-import still gets them.
  store.set({ summaryView: finalSummary, phase: "done", running: false, runDir });
}

/**
 * End a run the person gave up on, by a Cancel at a Review or a Discard of a
 * paused run: close it on the server as cancelled, with the Import Errors and
 * notes its record holds (`issuesToDiscard`, `notesToDiscard`), and delete
 * its run directory.
 *
 * The record is in the directory, so it is read before the directory goes, and a
 * record that cannot be read discards the run with no Import Errors. The
 * delete then runs whatever the close's outcome, but one: a session the
 * server refused to the close. The run stays open for the account's next
 * login to offer again, so its directory stays with it. Otherwise a live run
 * with no directory blocks the next import, and a directory with no run is
 * litter nothing will ever clean up. `runDir` is null for a run whose
 * directory is not on this device. Never throws.
 */
export async function discardRun(runId: number | null, runDir: string | null): Promise<void> {
  let issues: ImportIssue[] = [];
  let notes: ImportNote[] = [];
  if (runDir != null) {
    await recordWritesSettled();
    try {
      const record = parseRunRecord(await invokeReadImportRunRecord({ run_dir: runDir }));
      issues = issuesToDiscard(record);
      notes = notesToDiscard(record);
    } catch {
      // Discarded with no Import Errors: the run still has to close.
    }
  }
  if (runId != null) {
    try {
      await endSessionIfRefused(() => discardImportRun(runId, issueRequests(issues), notes));
    } catch (e: unknown) {
      if (e instanceof SessionRefusedError) return;
    }
  }
  if (runDir != null) await discardRunDirectory(runDir);
}
