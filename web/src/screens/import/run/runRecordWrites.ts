import { invokeSaveImportRunRecord, type UploadFinishedReport } from "../../../lib/tauri";
import { importRunStore as store } from "../importRunStore";
import { type RunRecord, recordToCarry } from "../runRecord";
import { currentPart, runScratch } from "./scratch";

/** A write of the run record waiting for the one before it to finish. */
type RecordWrite = { runDir: string; build: () => RunRecord };

/** Every write of the run record so far, in order: settles when the last has. */
let recordWrites: Promise<void> = Promise.resolve();

/** The write queued behind the one in progress, not yet started. */
let queuedRecordWrite: RecordWrite | null = null;

/** Settles when every write of the run record asked for so far has. */
export function recordWritesSettled(): Promise<void> {
  return recordWrites;
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
function writeRunRecord(runDir: string, build: () => RunRecord): Promise<void> {
  if (queuedRecordWrite?.runDir === runDir) {
    queuedRecordWrite.build = build;
    return recordWrites;
  }
  const write: RecordWrite = { runDir, build };
  queuedRecordWrite = write;
  recordWrites = recordWrites.then(async () => {
    if (queuedRecordWrite === write) queuedRecordWrite = null;
    try {
      await invokeSaveImportRunRecord({ run_dir: write.runDir, record: write.build() });
    } catch {
      // Nothing to show; see above.
    }
  });
  return recordWrites;
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
  report: UploadFinishedReport | null = null,
  uploadMs: number | null = null,
): Promise<void> {
  const { runDir } = store.get();
  if (runDir == null) return;
  const run = runScratch();
  await writeRunRecord(runDir, () =>
    recordToCarry(run.carried, currentPart(report, uploadMs, run)),
  );
}
