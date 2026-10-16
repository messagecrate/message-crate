import { invokeReadLog, invokeReadLogLines, type LogReader } from "../../lib/desktop/runLog";
import { type LogLinesPage, listServerLogLines } from "../../lib/serverApi";

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
 * A log the viewer reads: the server's, or one in this computer's Logs Directory.
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

/**
 * One log in this computer's Logs Directory, an Import Run's or the desktop
 * app's own, read by `reader` through the desktop app.
 */
export function desktopLogSource(reader: LogReader, name: string): LogSource {
  return {
    key: ["logs", "desktop", "lines", reader.messageCrateId, reader.owner, name],
    // The desktop app cannot cancel the read, so the signal goes unused.
    readLines: (request) => invokeReadLogLines(reader, name, request),
  };
}

/**
 * What the viewer says for a run log whose lines have no time and level, in
 * place of the lines: it was written before they carried them.
 */
export const UNREADABLE_RUN_LOG =
  "This log was written before its lines carried a time and a level, so it cannot be shown here. Download it to read it.";

/** The download of one log in this computer's Logs Directory, whole, under its own name. */
export function desktopLogDownload(reader: LogReader, name: string): LogDownload {
  return { name, read: () => invokeReadLog(reader, name) };
}
