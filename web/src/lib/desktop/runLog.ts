import { invoke } from "@tauri-apps/api/core";
import type { LogLinesPage } from "../serverApi";

/** Who ran an Import Run, on which Message Crate: the first line of the run's log. */
interface RunLogAccount {
  importRunId: number;
  accountId: number;
  /** The server's address, for a person to read. */
  server: string;
  /**
   * The Message Crate's id, as `GET /v1/server` answers it: what tells two
   * Message Crates at one address apart.
   */
  messageCrateId: string;
}

/**
 * Start a new Import Run's log with the line that names the account running
 * it and the server, so the Logs panel shows the log to that account and the
 * owner only.
 */
export async function invokeStartImportRunLog(
  runDir: string,
  account: RunLogAccount,
): Promise<void> {
  return invoke("start_import_run_log", { args: { runDir, account } });
}

/** Who is asking for the logs on this computer. */
export interface RunLogReader {
  /** The id of the Message Crate the window is signed in to. */
  messageCrateId: string;
  accountId: number;
  /** The owner reads every run log; an account only those of its own runs. */
  owner: boolean;
}

/** One log on this computer: an Import Run's, or the desktop app's own. */
export interface LogEntry {
  /** The file's name in the Logs Directory, which a download keeps. */
  name: string;
  /**
   * An Import Run's log, or the desktop app's own: what it did outside an
   * Import Run, which names no account and so is listed for the owner alone.
   */
  kind: "importRun" | "desktopApp";
  /** Who ran the run, or null when the log does not say. */
  account: RunLogAccount | null;
  /** Whether the run imported into the Message Crate the reader is signed in to. */
  thisMessageCrate: boolean;
  /**
   * Whether its first lines have a time and a level. A log written before
   * its lines carried them has none the viewer reads, though it downloads.
   */
  hasLines: boolean;
  bytes: number;
  /** When its last line was written, in UTC (RFC 3339). */
  modifiedAt: string;
}

/**
 * The logs on this computer that `reader` may read, newest first: its Import
 * Run logs, and for the owner every one and the desktop app's own.
 */
export async function invokeListLogs(reader: RunLogReader): Promise<LogEntry[]> {
  return invoke("list_logs", { reader });
}

/**
 * The levels a run log writes. The desktop app refuses `debug` and `trace`,
 * which only the server's log has.
 */
type RunLogLevel = "error" | "warn" | "info";

/**
 * A page of one log's lines, newest first: those at `level` and
 * more severe (every line when absent), holding `text`, older than the line
 * whose id is `after`. The page has the shape the server's log answers.
 */
export async function invokeReadLogLines(
  reader: RunLogReader,
  name: string,
  query: {
    level?: RunLogLevel;
    text?: string;
    after?: number;
    limit: number;
  },
): Promise<LogLinesPage> {
  return invoke("read_log_lines", { args: { reader, name, query } });
}

/** One log whole, byte for byte as it is on disk, for a download. */
export async function invokeReadLog(reader: RunLogReader, name: string): Promise<ArrayBuffer> {
  return invoke("read_log", { args: { reader, name } });
}

/**
 * The log of the Import Run whose directory is `runDir`, in the Logs
 * Directory, where it stays after the run's directory is deleted.
 */
export async function invokeImportRunLog(runDir: string): Promise<string> {
  return invoke("import_run_log", { runDir });
}
