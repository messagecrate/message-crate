import type { ClosedImportStatus, RunDirOfImportRun } from "../../lib/desktop/runDir";
import type { ImportStatus } from "../../lib/importRun";

/** What deleting the closed runs' directories reads and does. */
export interface ClosedRunDirectories {
  /** The account's run directories on this computer, each tied to its run. */
  list: () => Promise<RunDirOfImportRun[]>;
  /** How the server reports the Import Run with this id stands. */
  status: (importRunId: number) => Promise<ImportStatus>;
  /** Delete the directory of a run the server reports as closed. */
  deleteClosed: (run: RunDirOfImportRun, status: ClosedImportStatus) => Promise<boolean>;
}

/**
 * Delete the directory of every Import Run of the account that the server
 * reports as closed (#2822). A closed run's files are never read again, and
 * a directory can outlive its run, as when a discard commits after the
 * window has read the run as still running.
 *
 * A run still running keeps its directory, since Resume reads it. A run whose
 * state the server does not report, because it is unreachable or refuses,
 * keeps it too: nothing says the run is closed. The desktop process lists
 * only directories whose log ties them to one of the account's runs, so a
 * directory it cannot tie to a run is never offered here.
 *
 * `going` says whether the screen still wants the work: once it answers
 * false, nothing more is asked or deleted.
 */
export async function deleteClosedRunDirectories(
  calls: ClosedRunDirectories,
  going: () => boolean = () => true,
): Promise<void> {
  let runs: RunDirOfImportRun[];
  try {
    runs = await calls.list();
  } catch (e) {
    console.warn("The Import Run directories on this computer could not be listed:", e);
    return;
  }
  for (const run of runs) {
    if (!going()) return;
    let status: ImportStatus;
    try {
      status = await calls.status(run.importRunId);
    } catch {
      // Unreachable, or refused: the run's state is not known.
      continue;
    }
    if (status === "running" || !going()) continue;
    try {
      await calls.deleteClosed(run, status);
    } catch (e) {
      console.warn(`The directory of Import Run ${run.importRunId} could not be deleted:`, e);
    }
  }
}
