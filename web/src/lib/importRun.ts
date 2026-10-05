import {
  discardImport,
  listEveryImport,
  listImports,
  setImportStage as setStage,
} from "./serverApi";
import type { components } from "./serverApi.types";
import { invokePathStat, type PathStat } from "./tauri";

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

/**
 * The directories of the account's Import Runs that are on this
 * computer, for deleting with the account (#1491).
 *
 * The server keeps where each run staged its files, but the directories are on
 * whichever computer ran it, so each one is looked for here and only the
 * directories found are named. Desktop app only: it asks the app for each path.
 */
export async function accountRunDirectories(signal?: AbortSignal): Promise<string[]> {
  const runs = await listEveryImport({ signal });
  const paths = [...new Set(runs.flatMap((run) => (run.run_dir ? [run.run_dir] : [])))];
  const found = await Promise.all(
    paths.map(async (path) => {
      const stat = await invokePathStat(path);
      return stat.exists && stat.isDirectory ? path : null;
    }),
  );
  return found.filter((path): path is string => path !== null);
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
 */
export function buildSourceFingerprint(path: string, stat: PathStat): SourceFingerprint {
  return {
    path,
    size_bytes: stat.sizeBytes,
    modified_unix_ms: stat.modifiedUnixMs,
  };
}
