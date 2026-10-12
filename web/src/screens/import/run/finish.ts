import type {
  ImportIssue,
  ImportNote,
  ImportSummaryView,
} from "../../../components/import/ImportSummaryPanel";
import { ApiError } from "../../../lib/api";
import { errorLine, errorText } from "../../../lib/apiErrorMessage";
import type { UploadFinishedReport } from "../../../lib/tauri";
import { STAGING_LABEL, UPLOAD_LABEL } from "../importProgressState";
import {
  filesSkippedOverRun,
  issueRequests,
  issuesToDiscard,
  noteRequests,
  notesToDiscard,
  parseRunRecord,
  RUN_ERROR_ITEM,
  wholeRun,
} from "../runRecord";
import type { RunContext } from "./context";
import { recordError } from "./desktopJob";
import { discardRunDirectory } from "./runDirectory";
import { recordWritesSettled, saveCarriedRecord } from "./runRecordWrites";
import { currentPart } from "./scratch";
import { failActiveStep, movePhase, setRowByLabel, updateSteps } from "./screen";
import { endSessionIfRefused, leaveIfRefused, SessionRefusedError } from "./serverCalls";

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
export async function finishImport(
  ctx: RunContext,
  args: {
    runId: number | null;
    status: "completed" | "completed_with_issues" | "failed" | "cancelled" | "paused";
    uploadReport: UploadFinishedReport | null;
    uploadMs: number | null;
    skipComplete?: boolean;
  },
): Promise<void> {
  const { runId, status, uploadReport, uploadMs, skipComplete } = args;
  const carried = ctx.scratch.carried;
  const whole = wholeRun(carried, currentPart(ctx, uploadReport, uploadMs));
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
  updateSteps(ctx, (current) =>
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
      await endSessionIfRefused(ctx, () =>
        ctx.server.completeImport(runId, {
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
          notes: noteRequests(whole.notes ?? []),
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
      setRowByLabel(ctx, UPLOAD_LABEL, { status: "error", detail: "Paused" });
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
  let runDir = ctx.store.get().runDir;
  if (runEnded) {
    // An ended run's directory goes: the staged messages, the Upload's log, journal
    // and report, and the run record. When the delete fails, the directory link
    // stays and the failure is shown, so the person can find what was left
    // and remove it by hand. A record write still on its way finishes
    // first, so it cannot land in the directory after the delete.
    await recordWritesSettled(ctx);
    if (runDir != null && (await discardRunDirectory(ctx, runDir))) runDir = null;
  } else {
    await saveCarriedRecord(ctx, uploadReport, uploadMs);
  }
  // The server writes this run's saved search and Contact Group when the run
  // completes, so a window closed mid-import still gets them.
  movePhase(ctx, "finish", { summaryView: finalSummary, running: false, runDir });
}

/**
 * Stop `stage` when the server did not record its start (`moveStage` threw
 * `e`). A refused session leaves the run for the form (`leaveIfRefused`).
 * Otherwise the server still has the run at the review before `stage`, so the
 * run stays there and is not completed: a later visit offers that review
 * again.
 */
export async function stopStageNotRecorded(
  ctx: RunContext,
  e: unknown,
  runId: number,
  stage: "media" | "upload",
): Promise<void> {
  if (await leaveIfRefused(ctx, e, runId)) return;
  recordError(ctx, stage, errorLine(e));
  failActiveStep(ctx);
  await finishImport(ctx, {
    runId,
    status: "failed",
    uploadReport: null,
    uploadMs: null,
    skipComplete: true,
  });
}

/**
 * How a `discardRun` went: the run is closed on the server, the server still
 * holds it running (with the server's message, or why the close never
 * reached it), or the server refused the session the close was sent with.
 */
export type DiscardOutcome =
  | { kind: "discarded" }
  | { kind: "not_closed"; message: string }
  | { kind: "session_ended" };

/**
 * True when the server's answer to a discard says it holds no running run
 * by that id for the account: `404 Not Found` (`not-found`), or
 * `409 Conflict` (`state-conflict`), which this route gives only for a run
 * that is no longer running. Another window, or a close whose answer was
 * lost, may have closed it. Read by problem type, so a 404 from something
 * in front of the server, which never reached it, is not taken for one.
 */
function runAlreadyClosed(e: unknown): boolean {
  return e instanceof ApiError && (e.type === "not-found" || e.type === "state-conflict");
}

/**
 * True when a call failed without the server saying why: the request never
 * left, the connection dropped, its time limit ended it, something in front
 * of the server answered instead, such as a proxy's `504 Gateway Timeout`,
 * or the server failed with a `500 Internal Server Error` (`about:blank`).
 * Every other failure the server answers carries a problem type. In each of
 * these the server may still have done what the call asked.
 */
function answerLost(e: unknown): boolean {
  return !(e instanceof ApiError) || e.type === null;
}

/**
 * How a discard of `runId` that failed with `e` went: `discarded` when the
 * run is closed all the same. A session the server refused, to the discard
 * or to the read below, is `session_ended`. A server that says it holds no
 * such running run (`runAlreadyClosed`) has closed it. A discard whose answer
 * was lost (`answerLost`) may have been committed although no answer from
 * the server arrived (#2807), so the account's running run is read again,
 * and the run is closed when it is no longer that one. A failed read, or any
 * other refusal, leaves it running: `not_closed`, with the discard's message.
 */
async function outcomeOfFailedDiscard(
  ctx: RunContext,
  runId: number,
  e: unknown,
): Promise<DiscardOutcome> {
  const discarded: DiscardOutcome = { kind: "discarded" };
  if (e instanceof SessionRefusedError) return { kind: "session_ended" };
  if (runAlreadyClosed(e)) return discarded;
  const notClosed: DiscardOutcome = { kind: "not_closed", message: errorText(e) };
  if (!answerLost(e)) return notClosed;
  try {
    const running = await endSessionIfRefused(ctx, () => ctx.server.getActiveImportRun());
    return running?.id === runId ? notClosed : discarded;
  } catch (readError: unknown) {
    return readError instanceof SessionRefusedError ? { kind: "session_ended" } : notClosed;
  }
}

/**
 * End a run the person gave up on, by a Cancel at a Review or a Discard of a
 * paused run: close it on the server as cancelled, with the Import Errors and
 * notes its record holds (`issuesToDiscard`, `notesToDiscard`), and delete
 * its run directory.
 *
 * The record is in the directory, so it is read before the directory goes, and a
 * record that cannot be read discards the run with no Import Errors. The
 * directory goes only once the run is closed on the server (#2669): the
 * close went through, the server says the run is no longer running
 * (`runAlreadyClosed`), or a close whose answer was lost is followed by a
 * read that finds the run no longer running (`outcomeOfFailedDiscard`). Any
 * other refusal, or a close that never reached the server, leaves the run
 * open there, holding the account's import slot, so
 * its directory stays with it for the next visit to Import to offer again,
 * and the outcome carries the message for the caller to show. A session
 * the server refused to the close is no fault of the run: the run stays for
 * the account's next login, with its directory and no message. `runDir` is
 * null for a run whose directory is not on this device. Never throws.
 */
export async function discardRun(
  ctx: RunContext,
  runId: number | null,
  runDir: string | null,
): Promise<DiscardOutcome> {
  let issues: ImportIssue[] = [];
  let notes: ImportNote[] = [];
  if (runDir != null) {
    await recordWritesSettled(ctx);
    try {
      const record = parseRunRecord(
        await ctx.desktop.invokeReadImportRunRecord({ run_dir: runDir }),
      );
      issues = issuesToDiscard(record);
      notes = notesToDiscard(record);
    } catch {
      // Discarded with no Import Errors: the run still has to close.
    }
  }
  if (runId != null) {
    try {
      await endSessionIfRefused(ctx, () =>
        ctx.server.discardImportRun(runId, issueRequests(issues), noteRequests(notes)),
      );
    } catch (e: unknown) {
      const outcome = await outcomeOfFailedDiscard(ctx, runId, e);
      if (outcome.kind !== "discarded") return outcome;
    }
  }
  if (runDir != null) await discardRunDirectory(ctx, runDir);
  return { kind: "discarded" };
}
