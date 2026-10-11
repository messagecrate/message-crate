import { errorText } from "./apiErrorMessage";
import { unknownPathMessage } from "./pathChecks";
import {
  discardImport,
  listEveryImport,
  listImports,
  setImportStage as setStage,
} from "./serverApi";
import type { components } from "./serverApi.types";
import { invokeDeleteRunDir, invokePathStat, type PathStat } from "./tauri";

/** Where a running Import Run is: the server's `ImportStage`. */
export type ImportStage = components["schemas"]["ImportStage"];

/** Identity of the backup an Import Run was started from. */
export type SourceFingerprint = {
  path: string;
  size_bytes: number;
  modified_unix_ms: number | null;
};

/** The account's running Import Run, as the server reports it. */
export type ActiveImportRun = {
  id: number;
  source: string;
  mode: string;
  status: string;
  started_at: string;
  stage: ImportStage | null;
  run_dir: string | null;
  device_id: string | null;
  form: unknown;
  source_fingerprint: SourceFingerprint | null;
  /** Addresses the backup's device sent from (JSON array), or null. */
  source_identities: unknown;
  /** What was approved at the last Review, or null. Mirrors what
   * `setImportStage`'s `approvedPlan` argument last wrote. */
  summary: unknown;
};

/**
 * The account's running Import Run, or null when there is none. At most one
 * runs at a time, so the first item of `status=running` is the one.
 */
export async function getActiveImportRun(signal?: AbortSignal): Promise<ActiveImportRun | null> {
  const run = (await listImports({ status: "running", limit: 1 }, { signal })).items[0];
  if (!run) return null;
  return {
    ...run,
    stage: run.stage ?? null,
    run_dir: run.run_dir ?? null,
    device_id: run.device_id ?? null,
    source_fingerprint: run.source_fingerprint as SourceFingerprint | null,
  };
}

/** An Import Run's directory Message Crate left in place, and why. */
export type DirectoryReason = { path: string; reason: string };

/**
 * The directories of the account's Import Runs on this computer: `found`,
 * the ones to delete with the account, and `unreadable`, the ones the
 * operating system would not let Message Crate read. Those may be here with
 * the account's data, or the run may have been on another computer. Message
 * Crate cannot tell, and cannot delete them either, so they are named with
 * the system's reason once the account is deleted (#2651).
 */
export type AccountRunDirectories = {
  found: string[];
  unreadable: DirectoryReason[];
};

/**
 * The directories of the account's Import Runs that are on this
 * computer, for deleting with the account (#1491).
 *
 * The server keeps where each run staged its files, but the directories are on
 * whichever computer ran it, so each one is looked for here and only the
 * directories found are named. A directory the operating system would not
 * describe is not taken for missing: it comes back in `unreadable`.
 * Desktop app only: it asks the app for each path.
 */
export async function accountRunDirectories(signal?: AbortSignal): Promise<AccountRunDirectories> {
  const runs = await listEveryImport({ signal });
  const paths = [...new Set(runs.flatMap((run) => (run.run_dir ? [run.run_dir] : [])))];
  const stats = await Promise.all(
    paths.map(async (path) => ({ path, stat: await invokePathStat(path) })),
  );
  const directories: AccountRunDirectories = { found: [], unreadable: [] };
  for (const { path, stat } of stats) {
    if (stat.unreadable) {
      directories.unreadable.push({ path, reason: unknownPathMessage(stat.unreadable, "it") });
    } else if (stat.exists && stat.isDirectory) {
      directories.found.push(path);
    }
  }
  return directories;
}

/**
 * Delete the directories of a deleted account's Import Runs, one at a time,
 * and return the ones that could not be deleted. Desktop app only.
 */
export async function deleteRunDirectories(
  directories: readonly string[],
): Promise<DirectoryReason[]> {
  const undeleted: DirectoryReason[] = [];
  for (const path of directories) {
    try {
      await invokeDeleteRunDir({ run_dir: path });
    } catch (e) {
      undeleted.push({ path, reason: errorText(e) });
    }
  }
  return undeleted;
}

/**
 * Move a running Import Run to another stage.
 *
 * `approvedPlan`, when given, is recorded as the run's `summary_json` —
 * what the user approved at the Review the run just left. Omitting it leaves
 * whatever plan is already stored untouched; it is never nulled out.
 */
export async function setImportStage(
  id: number,
  stage: ImportStage,
  approvedPlan?: unknown,
): Promise<void> {
  await setStage(id, { stage, summary: approvedPlan });
}

/**
 * Close an Import Run the user gave up on, freeing the account's slot. The run is
 * recorded as cancelled with `issues`, the Import Errors it recorded before,
 * and `notes`.
 */
export async function discardImportRun(
  id: number,
  issues: components["schemas"]["ImportIssueRequest"][],
  notes: components["schemas"]["ImportNoteRequest"][],
): Promise<void> {
  await discardImport(id, { issues, notes });
}

/**
 * Identity of the backup this run reads.
 *
 * The size and mtime come from a stat of the path itself, so for a
 * directory source -- an iOS backup directory, a WhatsApp directory -- they
 * describe the directory entry rather than its contents, and neither moves
 * when a file inside it grows. `checkSourceFingerprint` reads this
 * fingerprint back on resume, so a change inside a directory goes unseen there.
 *
 * Null for a backup the operating system would not describe: its size and
 * modified time are unknown, and recording zeros would read as a changed
 * backup once the app may read it.
 */
export function buildSourceFingerprint(path: string, stat: PathStat): SourceFingerprint | null {
  if (stat.unreadable) return null;
  return {
    path,
    size_bytes: stat.sizeBytes,
    modified_unix_ms: stat.modifiedUnixMs,
  };
}
