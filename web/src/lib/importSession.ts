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

/** Identity of the backup a session was started from. */
export type SourceFingerprint = {
  path: string;
  size_bytes: number;
  modified_unix_ms: number | null;
  /** Always null: nothing fills it in. */
  message_count: number | null;
};

/** The account's live import session, as the server reports it. */
export type ActiveImportSession = {
  id: number;
  source: string;
  mode: string;
  status: string;
  started_at: string;
  stage: ImportStage | null;
  staging_dir: string | null;
  device_id: string | null;
  form: unknown;
  source_fingerprint: SourceFingerprint | null;
  /** Addresses the backup's device sent from (JSON array), or null. */
  source_identities: unknown;
  /** What was approved at the last gate passed, or null. Mirrors what
   * `setImportStage`'s `approvedPlan` argument last wrote. */
  summary: unknown;
};

/**
 * The account's running Import Run, or null when there is none. At most one
 * runs at a time, so the first item of `status=running` is the one.
 */
export async function getActiveImportSession(
  signal?: AbortSignal,
): Promise<ActiveImportSession | null> {
  const session = (await listImports({ status: "running", limit: 1 }, { signal })).items[0];
  if (!session) return null;
  return {
    ...session,
    stage: session.stage ?? null,
    staging_dir: session.staging_dir ?? null,
    device_id: session.device_id ?? null,
    source_fingerprint: session.source_fingerprint as SourceFingerprint | null,
  };
}

/**
 * The Staging Directories of the account's Import Runs that are on this
 * computer, for deleting with the account (#1491).
 *
 * The server keeps where each run staged its files, but the directories are on
 * whichever computer ran it, so each one is looked for here and only the
 * directories found are named. Desktop app only: it asks the app for each path.
 */
export async function accountStagingDirectories(signal?: AbortSignal): Promise<string[]> {
  const runs = await listEveryImport({ signal });
  const paths = [...new Set(runs.flatMap((run) => (run.staging_dir ? [run.staging_dir] : [])))];
  const found = await Promise.all(
    paths.map(async (path) => {
      const stat = await invokePathStat(path);
      return stat.exists && stat.isDirectory ? path : null;
    }),
  );
  return found.filter((path): path is string => path !== null);
}

/**
 * Move a live session to another stage.
 *
 * `approvedPlan`, when given, is recorded as the session's `summary_json` —
 * what the user approved at the gate they just passed. Omitting it leaves
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
 * Close a session the user gave up on, freeing the account's slot. The run is
 * recorded as cancelled with `issues`, the Import Errors it recorded before,
 * and `notes`.
 */
export async function discardImportSession(
  id: number,
  issues: components["schemas"]["ImportIssueRequest"][],
  notes: components["schemas"]["ImportNoteRequest"][],
): Promise<void> {
  await discardImport(id, { issues, notes });
}

/**
 * Identity of the backup this session reads.
 *
 * The message count starts null, and nothing fills it in after parse.
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
    message_count: null,
  };
}
