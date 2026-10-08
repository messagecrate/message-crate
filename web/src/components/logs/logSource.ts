import { getBaseUrl } from "../../lib/api";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { useAuth } from "../../lib/auth";
import { canReadImportRunLogs } from "../../lib/desktopFeatures";
import { useRouteQuery } from "../../lib/routeQuery";
import {
  getServerLogFile,
  type LogLevel,
  type LogLinesPage,
  listServerLogFiles,
  listServerLogLines,
} from "../../lib/serverApi";
import {
  invokeListImportRunLogs,
  invokeReadImportRunLog,
  invokeReadImportRunLogLines,
  type RunLogEntry,
  type RunLogReader,
} from "../../lib/tauri";
import { isTauri } from "../../lib/tauri-check";
import { useIsOwner } from "../../lib/useIsOwner";

/** What the level filter shows: errors, warnings and up, or every line. */
export type LevelFilter = "error" | "warn" | "all";

/** What a page of lines asks for. */
export type LinesRequest = {
  level?: LogLevel;
  text?: string;
  after?: number;
  limit: number;
};

/** One file a log downloads as, read whole when it is downloaded. */
export type LogDownload = {
  name: string;
  read: () => Promise<string>;
};

/**
 * A log the viewer reads: the server's, or one Import Run's on this computer.
 * The viewer reads both the same way, a page of lines at a time, newest first.
 */
export type LogSource = {
  /** Names this log in the query cache, after the account. */
  key: readonly unknown[];
  readLines: (request: LinesRequest, signal: AbortSignal) => Promise<LogLinesPage>;
};

/** The server's log, read through `/v1/server/log-lines`. The owner's alone. */
export const SERVER_LOG: LogSource = {
  key: ["logs", "server"],
  readLines: (request, signal) => listServerLogLines(request, { signal }),
};

/** One Import Run's log on this computer, read by `reader`. */
export function runLogSource(reader: RunLogReader, name: string): LogSource {
  return {
    key: ["logs", "run", reader.server, reader.owner, name],
    // The desktop app cannot cancel the read, so the signal goes unused.
    readLines: (request) => invokeReadImportRunLogLines(reader, name, request),
  };
}

/** The download of one Import Run's log, whole, under its own name. */
export function runLogDownload(reader: RunLogReader, name: string): LogDownload {
  return { name, read: () => invokeReadImportRunLog(reader, name) };
}

/**
 * The files of the server's log, each a download, newest first. The owner's
 * alone, like the lines.
 */
export function useServerLogDownloads(): { downloads: LogDownload[]; error: Error | null } {
  const files = useRouteQuery(["logs", "server", "files"], (signal) =>
    listServerLogFiles({ signal }),
  );
  return {
    downloads: (files.data ?? []).map((file) => ({
      name: file.name,
      read: () => getServerLogFile(file.id),
    })),
    error: files.error,
  };
}

/**
 * Who reads the Import Run logs on this computer: the signed-in account on
 * the server it is signed in to, and whether it is the owner, who reads every
 * one. Null in a browser, where no run log is, and before the profile says
 * whether the account is the owner.
 */
export function useRunLogReader(): RunLogReader | null {
  const { accountId } = useAuth();
  const { isOwner, loading } = useIsOwner();
  if (!canReadImportRunLogs(isTauri()) || accountId === null || loading) return null;
  return { server: getBaseUrl(), accountId, owner: isOwner };
}

/** The Import Run logs on this computer `reader` may read, newest first. */
export function useRunLogs(reader: RunLogReader | null): {
  logs: RunLogEntry[];
  loading: boolean;
  error: Error | null;
} {
  const listing = useRouteQuery(
    ["logs", "runs", reader?.server, reader?.owner],
    () => (reader ? invokeListImportRunLogs(reader) : Promise.resolve([])),
    { enabled: reader !== null },
  );
  return {
    logs: listing.data ?? [],
    loading: reader !== null && listing.isPending,
    error: listing.error,
  };
}

/**
 * What went wrong reading a log, in a sentence: the server's own for its log,
 * and the desktop app's for a run log, which it rejects with as a string.
 */
export function logErrorMessage(err: unknown, fallback: string): string {
  return typeof err === "string" && err ? err : apiErrorMessage(err, fallback);
}
