import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { type DesktopJobName, holdDesktopJob } from "../desktopJob";
import { CanceledError, DesktopJobError } from "../desktopJobError";
import type {
  DesktopJobErrorEvent,
  ImportFileDoneEvent,
  ImportFileWrittenEvent,
  ImportIssueEvent,
  ImportProgressEvent,
} from "../types";
import type { TranscodeFinishedReport } from "./staging";
import type { UploadFinishedReport } from "./upload";

/** Ask the desktop backend to stop the job that is currently running. */
export async function invokeCancel(): Promise<void> {
  return invoke("cancel");
}

export interface DesktopJobResult {
  summary: string;
  report?: UploadFinishedReport;
  extraction?: {
    files_parsed: number;
    messages_parsed: number;
  };
  transcode?: TranscodeFinishedReport;
}

/**
 * Listen for job events from the desktop backend (log lines, progress, errors).
 * Returns one function that removes every listener.
 */
export function onDesktopJobEvents(callbacks: {
  onLog: (line: string) => void;
  onProgress?: (event: ImportProgressEvent) => void;
  onIssue?: (event: ImportIssueEvent) => void;
  onFileDone?: (event: ImportFileDoneEvent) => void;
  onFileWritten?: (event: ImportFileWrittenEvent) => void;
  onFinished: (summary: string) => void;
  onError: (err: DesktopJobErrorEvent) => void;
}): Promise<UnlistenFn> {
  return Promise.all([
    listen<string>("desktop-job:log", (e) => callbacks.onLog(e.payload)),
    listen<ImportProgressEvent>("desktop-job:progress", (e) => callbacks.onProgress?.(e.payload)),
    listen<ImportIssueEvent>("desktop-job:issue", (e) => callbacks.onIssue?.(e.payload)),
    listen<ImportFileDoneEvent>("desktop-job:file-done", (e) => callbacks.onFileDone?.(e.payload)),
    listen<ImportFileWrittenEvent>("desktop-job:file-written", (e) =>
      callbacks.onFileWritten?.(e.payload),
    ),
    listen<string>("desktop-job:finished", (e) => callbacks.onFinished(e.payload)),
    listen<DesktopJobErrorEvent>("desktop-job:error", (e) => callbacks.onError(e.payload)),
  ]).then((unlisteners) => {
    return () => {
      for (const u of unlisteners) {
        u();
      }
    };
  });
}

/**
 * Run a desktop job and wait until it finishes.
 * Extract and upload return as soon as the background thread starts, so callers
 * must use this instead of awaiting the invoke call alone.
 *
 * `job` names the screen's job while it runs (`desktopJob.ts`), so the other
 * screens keep their Start buttons off until it ends.
 */
export async function awaitDesktopJob(
  job: DesktopJobName,
  invokeFn: () => Promise<void>,
  onLog?: (line: string) => void,
  onProgress?: (event: ImportProgressEvent) => void,
  onIssue?: (event: ImportIssueEvent) => void,
  onFileDone?: (event: ImportFileDoneEvent) => void,
  onFileWritten?: (event: ImportFileWrittenEvent) => void,
): Promise<DesktopJobResult> {
  let unlisten: UnlistenFn | undefined;
  const release = holdDesktopJob(job);
  try {
    return await new Promise<DesktopJobResult>((resolve, reject) => {
      void (async () => {
        try {
          unlisten = await onDesktopJobEvents({
            onLog: (line) => onLog?.(line),
            onProgress,
            onIssue,
            onFileDone,
            onFileWritten,
            onFinished: (summary) => resolve(parseDesktopJobResult(summary)),
            onError: (err) => reject(desktopJobError(err, onLog)),
          });
          await invokeFn();
        } catch (e: unknown) {
          reject(e instanceof Error ? e : new Error(String(e)));
        }
      })();
    });
  } finally {
    unlisten?.();
    release();
  }
}

/**
 * What `awaitDesktopJob` rejects with for a job's `desktop-job:error`: a
 * `CanceledError` when a Cancel stopped it, a `DesktopJobError` when the
 * desktop app sent a sentence for the person, and otherwise an `Error` with
 * the chain. A `DesktopJobError`'s chain also goes to the job's log, so a
 * panic's text is not lost (#2071, #2112).
 */
function desktopJobError(err: DesktopJobErrorEvent, onLog?: (line: string) => void): Error {
  if (err.canceled) return new CanceledError();
  if (err.user_message === undefined) return new Error(err.detail);
  onLog?.(err.detail);
  return new DesktopJobError(err.user_message, err.detail);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isUploadFinishedReport(value: unknown): value is UploadFinishedReport {
  if (!isRecord(value)) return false;
  return (
    typeof value.ok === "boolean" &&
    typeof value.canceled === "boolean" &&
    typeof value.session_refused === "boolean" &&
    typeof value.messages_attempted === "number" &&
    typeof value.messages_inserted === "number" &&
    typeof value.messages_deduped === "number" &&
    typeof value.messages_failed === "number" &&
    typeof value.assets_uploaded === "number" &&
    typeof value.assets_bytes === "number" &&
    typeof value.conversations_ok === "number" &&
    typeof value.conversations_total === "number" &&
    typeof value.conversations_failed === "number" &&
    typeof value.conversations_skipped === "number" &&
    typeof value.conversations_canceled === "number"
  );
}

function isTranscodeFinishedReport(value: unknown): value is TranscodeFinishedReport {
  if (!isRecord(value)) return false;
  return (
    typeof value.converted === "number" &&
    typeof value.skipped === "number" &&
    typeof value.too_large === "number" &&
    typeof value.failed === "number" &&
    typeof value.missing === "number" &&
    typeof value.repointed === "number" &&
    typeof value.bytes_before === "number" &&
    typeof value.bytes_after === "number"
  );
}

/** Turn a finished-job summary string into a structured result when it is JSON. */
export function parseDesktopJobResult(summary: string): DesktopJobResult {
  try {
    const parsed: unknown = JSON.parse(summary);
    if (!isRecord(parsed)) return { summary };

    if (
      typeof parsed.summary === "string" &&
      typeof parsed.files_parsed === "number" &&
      typeof parsed.messages_parsed === "number"
    ) {
      return {
        summary: parsed.summary,
        extraction: {
          files_parsed: parsed.files_parsed,
          messages_parsed: parsed.messages_parsed,
        },
      };
    }

    const summaryText = typeof parsed.summary === "string" ? parsed.summary : summary;

    if (isUploadFinishedReport(parsed)) {
      return {
        summary: summaryText,
        report: parsed,
      };
    }

    if (isTranscodeFinishedReport(parsed)) {
      // Picked field by field, not spread: `parsed` also carries the raw
      // `summary` string this same object holds the report fields
      // alongside, which isn't part of `TranscodeFinishedReport`.
      return {
        summary: summaryText,
        transcode: {
          converted: parsed.converted,
          skipped: parsed.skipped,
          too_large: parsed.too_large,
          failed: parsed.failed,
          missing: parsed.missing,
          repointed: parsed.repointed,
          bytes_before: parsed.bytes_before,
          bytes_after: parsed.bytes_after,
        },
      };
    }
  } catch {
    // Format and export jobs send a plain sentence, not JSON.
  }
  return { summary };
}
