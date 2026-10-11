import { useAuth } from "../../lib/authContext";
import { canReadImportRunLogs } from "../../lib/desktopFeatures";
import { useRouteQuery } from "../../lib/routeQuery";
import {
  getServerLogFile,
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
import { useServerInfo } from "../../lib/useServerInfo";

/** What the level filter shows: errors, warnings and up, or every line. */
export type LevelFilter = "error" | "warn" | "all";

/**
 * What a page of lines asks for. The level is one the filter offers, which
 * both the server's log and a run log read.
 */
export type LinesRequest = {
  level?: Exclude<LevelFilter, "all">;
  text?: string;
  after?: number;
  limit: number;
};

/**
 * One file a log downloads as, read whole when it is downloaded: the server's
 * as text, a run log's as its bytes.
 */
export type LogDownload = {
  name: string;
  read: () => Promise<string | ArrayBuffer>;
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
    key: ["logs", "run", reader.messageCrateId, reader.owner, name],
    // The desktop app cannot cancel the read, so the signal goes unused.
    readLines: (request) => invokeReadImportRunLogLines(reader, name, request),
  };
}

/**
 * What the viewer says for a run log whose lines have no time and level, in
 * place of the lines: it was written before they carried them.
 */
export const UNREADABLE_RUN_LOG =
  "This log was written before its lines carried a time and a level, so it cannot be shown here. Download it to read it.";

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
 * the Message Crate it is signed in to, and whether it is the owner, who reads
 * every one. Null in a browser, where no run log is, and before the profile
 * says whether the account is the owner and the server says its id.
 */
export function useRunLogReader(): RunLogReader | null {
  const { accountId } = useAuth();
  const { isOwner, loading } = useIsOwner();
  const server = useServerInfo();
  const messageCrateId = server.data?.id;
  if (!canReadImportRunLogs(isTauri()) || accountId === null || loading || !messageCrateId) {
    return null;
  }
  return { messageCrateId, accountId, owner: isOwner };
}

/** The Import Run logs on this computer `reader` may read, newest first. */
export function useRunLogs(reader: RunLogReader | null): {
  logs: RunLogEntry[];
  loading: boolean;
  error: Error | null;
} {
  const listing = useRouteQuery(
    ["logs", "runs", reader?.messageCrateId, reader?.owner],
    () => (reader ? invokeListImportRunLogs(reader) : Promise.resolve([])),
    { enabled: reader !== null },
  );
  return {
    logs: listing.data ?? [],
    loading: reader !== null && listing.isPending,
    error: listing.error,
  };
}
