import { invoke } from "@tauri-apps/api/core";
import type { ImportStatus } from "../importRun";

/**
 * Make a new run directory under the Staging Directory and return its path.
 * `label` is the Import source.
 */
export async function invokeCreateRunDir(label: string): Promise<string> {
  return invoke("create_run_dir", { label });
}

/**
 * Delete a run directory — the decline path's terminal action: closing an
 * review without approving deletes the directory outright.
 */
export async function invokeDeleteRunDir(config: { run_dir: string }): Promise<void> {
  return invoke("delete_run_dir", {
    args: { runDir: config.run_dir },
  });
}

/**
 * Read the Import Run record kept in a run directory
 * (`read_import_run_record`): what a paused run's earlier parts recorded.
 * Null when the directory holds none. The caller checks its shape.
 */
export async function invokeReadImportRunRecord(config: {
  run_dir: string;
}): Promise<unknown | null> {
  return invoke("read_import_run_record", {
    args: { runDir: config.run_dir },
  });
}

/** Write the Import Run record into its run directory (`save_import_run_record`). */
export async function invokeSaveImportRunRecord(config: {
  run_dir: string;
  record: unknown;
}): Promise<void> {
  return invoke("save_import_run_record", {
    args: { runDir: config.run_dir, record: config.record },
  });
}

/** A run directory on this computer, and the Import Run its log says it holds. */
export interface RunDirOfImportRun {
  runDir: string;
  importRunId: number;
}

/**
 * The run directories on this computer of `accountId`'s Import Runs on the
 * Message Crate `messageCrateId`, each tied to its run by the run's log. A
 * directory whose log names no run is not listed.
 */
export async function invokeListAccountRunDirs(
  accountId: number,
  messageCrateId: string,
): Promise<RunDirOfImportRun[]> {
  return invoke("list_account_run_dirs", { args: { accountId, messageCrateId } });
}

/** How a closed Import Run ended: every status the server reports but `running`. */
export type ClosedImportStatus = Exclude<ImportStatus, "running">;

/**
 * Delete the directory of an Import Run the server reports as closed. The
 * desktop process deletes it only when the run's log names that run, by
 * `accountId` on the Message Crate `messageCrateId`, and writes a line in the
 * log naming the run and the directory. Resolves to whether a directory was
 * removed.
 */
export async function invokeDeleteClosedRunDir(
  run: RunDirOfImportRun,
  signedIn: { accountId: number; messageCrateId: string },
  status: ClosedImportStatus,
): Promise<boolean> {
  return invoke("delete_closed_run_dir", {
    args: {
      runDir: run.runDir,
      importRunId: run.importRunId,
      accountId: signedIn.accountId,
      messageCrateId: signedIn.messageCrateId,
      status,
    },
  });
}
