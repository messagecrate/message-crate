import type { UploadFinishedReport } from "../../../lib/tauri";
import { type RunRecord, recordToCarry } from "../runRecord";
import type { RunContext } from "./context";
import { currentPart } from "./scratch";

/** A write of the run record waiting for the one before it to finish. */
export type RecordWrite = { runDir: string; build: () => RunRecord };

/** Settles when every write of the run record asked for so far has. */
export function recordWritesSettled(ctx: RunContext): Promise<void> {
  return ctx.recordWrites;
}

/**
 * Write a run record into its run directory, one write at a time and in the
 * order they were asked for, so an older record never lands over a newer one.
 * The record is built when its write starts, from the run as it stands then,
 * and a write asked for while another waits for the same directory replaces the
 * waiting one: issues that arrive while a write is in progress all go in the
 * next. A failed write is not shown: the run goes on, and a resume starts
 * from the record the directory already held.
 */
function writeRunRecord(ctx: RunContext, runDir: string, build: () => RunRecord): Promise<void> {
  if (ctx.queuedRecordWrite?.runDir === runDir) {
    ctx.queuedRecordWrite.build = build;
    return ctx.recordWrites;
  }
  const write: RecordWrite = { runDir, build };
  ctx.queuedRecordWrite = write;
  ctx.recordWrites = ctx.recordWrites.then(async () => {
    if (ctx.queuedRecordWrite === write) ctx.queuedRecordWrite = null;
    try {
      await ctx.desktop.invokeSaveImportRunRecord({
        run_dir: write.runDir,
        record: write.build(),
      });
    } catch {
      // Nothing to show; see above.
    }
  });
  return ctx.recordWrites;
}

/**
 * Write the run's record so far into its run directory, for the part
 * that resumes it. Called wherever the run stops with the run still open (at
 * a Review, and when `finishImport` leaves the run open), and while a stage
 * runs, as each issue arrives (`recordIssue`, `recordFileDone`,
 * `recordFileWritten`). A failed write loses only this part's record; the
 * run itself is unaffected.
 *
 * While a stage runs, the write leaves in the run directory every issue
 * the window had received: each stage sends its issues the moment it
 * records them, Staging says when it has written each conversation, and the
 * Upload says when it has sent each one, so a crash loses only what arrived
 * while the last write was on its way to disk. The record is the one a
 * pause now would leave (`recordToCarry`): an Upload's rows about a
 * conversation not yet on the server wait apart, as do Staging's rows about
 * a conversation not yet written, and an earlier pause's rows about a
 * conversation this part has since sent, or written, go.
 */
export async function saveCarriedRecord(
  ctx: RunContext,
  report: UploadFinishedReport | null = null,
  uploadMs: number | null = null,
): Promise<void> {
  const { runDir } = ctx.store.get();
  if (runDir == null) return;
  const run = ctx.scratch;
  await writeRunRecord(ctx, runDir, () =>
    recordToCarry(run.carried, currentPart(ctx, report, uploadMs, run)),
  );
}
