import { describe, expect, it, vi } from "vitest";
import { currentDesktopJob, holdDesktopJob } from "../desktopJob";
import { CanceledError, DesktopJobError } from "../desktopJobError";
import type { DesktopJobErrorEvent } from "../types";
import { awaitDesktopJob, parseDesktopJobResult } from "./job";
import type { UploadFinishedReport } from "./upload";

const listeners = vi.hoisted(() => new Map<string, (e: { payload: unknown }) => void>());

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, handler: (e: { payload: unknown }) => void) => {
    listeners.set(name, handler);
    return () => listeners.delete(name);
  },
}));

function reportJson(overrides: Partial<UploadFinishedReport> = {}): string {
  const report = {
    ok: true,
    canceled: false,
    session_refused: false,
    messages_attempted: 10,
    messages_inserted: 10,
    messages_deduped: 0,
    messages_failed: 0,
    assets_uploaded: 1,
    assets_bytes: 100,
    conversations_ok: 5,
    conversations_total: 5,
    conversations_failed: 0,
    conversations_skipped: 0,
    conversations_canceled: 0,
    results: [],
    ...overrides,
  };
  return JSON.stringify(report);
}

describe("parseDesktopJobResult", () => {
  it("attaches a report that carries every field the verdict depends on", () => {
    const result = parseDesktopJobResult(reportJson());
    expect(result.report).toBeDefined();
    expect(result.report?.conversations_failed).toBe(0);
    expect(result.report?.conversations_skipped).toBe(0);
  });

  // Regression for the last path back to the 2026-08-27 incident: a report
  // JSON blob missing conversations_failed/conversations_skipped must not
  // narrow to UploadFinishedReport, or importOutcome computes
  // `undefined === 0` -> false and reports "completed" for a run where
  // nothing landed.
  it("does not attach a report missing conversations_failed", () => {
    const parsed: Record<string, unknown> = JSON.parse(reportJson());
    delete parsed.conversations_failed;
    const result = parseDesktopJobResult(JSON.stringify(parsed));
    expect(result.report).toBeUndefined();
  });

  it("does not attach a report missing conversations_skipped", () => {
    const parsed: Record<string, unknown> = JSON.parse(reportJson());
    delete parsed.conversations_skipped;
    const result = parseDesktopJobResult(JSON.stringify(parsed));
    expect(result.report).toBeUndefined();
  });

  // Without `conversations_canceled`, an Upload that left conversations
  // unsent could read as finished.
  it("does not attach a report missing conversations_canceled", () => {
    const parsed: Record<string, unknown> = JSON.parse(reportJson());
    delete parsed.conversations_canceled;
    const result = parseDesktopJobResult(JSON.stringify(parsed));
    expect(result.report).toBeUndefined();
  });

  // Without `canceled`, a paused Upload would read as a failed one.
  it("does not attach a report missing canceled", () => {
    const parsed: Record<string, unknown> = JSON.parse(reportJson());
    delete parsed.canceled;
    const result = parseDesktopJobResult(JSON.stringify(parsed));
    expect(result.report).toBeUndefined();
  });

  it("still parses an extraction summary", () => {
    const result = parseDesktopJobResult(
      JSON.stringify({ summary: "done", files_parsed: 3, messages_parsed: 20 }),
    );
    expect(result.extraction).toEqual({ files_parsed: 3, messages_parsed: 20 });
  });

  it("falls back to a plain summary for a non-JSON string", () => {
    const result = parseDesktopJobResult("Extracted 10 messages.");
    expect(result).toEqual({ summary: "Extracted 10 messages." });
  });

  // transcode_staging's payload: TranscodeReport has no serde derive, so
  // staging.rs hand-builds the finished payload with these fields flat
  // alongside `summary`, snake_case, not nested under a `report` key. A
  // client that doesn't recognize this shape falls through to
  // `{ summary: <the raw JSON string> }`, which would render raw JSON
  // wherever the finished summary is displayed.
  it("recognizes the transcode-finished payload and returns typed counts, not raw JSON", () => {
    const summarySentence =
      "Converted 12 files; 2 will not be uploaded (still too large after conversion).";
    const payload = JSON.stringify({
      summary: summarySentence,
      converted: 12,
      skipped: 3,
      too_large: 2,
      failed: 1,
      missing: 0,
      repointed: 4,
      bytes_before: 900_000,
      bytes_after: 100_000,
    });

    const result = parseDesktopJobResult(payload);

    expect(result.summary).toBe(summarySentence);
    expect(result.summary).not.toContain("{");
    expect(result.transcode).toEqual({
      converted: 12,
      skipped: 3,
      too_large: 2,
      failed: 1,
      missing: 0,
      repointed: 4,
      bytes_before: 900_000,
      bytes_after: 100_000,
    });
    expect(result.report).toBeUndefined();
    expect(result.extraction).toBeUndefined();
  });

  it("does not mistake a transcode payload missing a count for one it recognizes", () => {
    const parsed: Record<string, unknown> = JSON.parse(
      JSON.stringify({
        summary: "Converted 1 file.",
        converted: 1,
        skipped: 0,
        too_large: 0,
        failed: 0,
        missing: 0,
        repointed: 0,
        bytes_before: 10,
        // bytes_after omitted
      }),
    );
    const result = parseDesktopJobResult(JSON.stringify(parsed));
    expect(result.transcode).toBeUndefined();
  });
});

describe("awaitDesktopJob", () => {
  it("holds the desktop job under its name until the job finishes", async () => {
    let seenWhileRunning: string | null = null;
    const done = awaitDesktopJob("Export", async () => {
      seenWhileRunning = currentDesktopJob();
      queueMicrotask(() => listeners.get("desktop-job:finished")?.({ payload: "Export complete" }));
    });
    await done;
    expect(seenWhileRunning).toBe("Export");
    expect(currentDesktopJob()).toBeNull();
  });

  it("leaves the hold of the run it is part of in place when it ends", async () => {
    // An Import Run or an Export holds the desktop across all its jobs; one
    // job ending must not release it before the next job starts (#1407).
    const releaseRun = holdDesktopJob("Import Run");
    try {
      await awaitDesktopJob("Import Run", async () => {
        queueMicrotask(() => listeners.get("desktop-job:finished")?.({ payload: "Staged" }));
      });
      expect(currentDesktopJob()).toBe("Import Run");
    } finally {
      releaseRun();
    }
    expect(currentDesktopJob()).toBeNull();
  });

  it("lets the desktop job go when the job fails", async () => {
    const done = awaitDesktopJob("Convert", async () => {
      queueMicrotask(() =>
        listeners.get("desktop-job:error")?.({ payload: { detail: "disk full", canceled: false } }),
      );
    });
    await expect(done).rejects.toThrow("disk full");
    expect(currentDesktopJob()).toBeNull();
  });

  /** A job that ends with `error` on `desktop-job:error`, and the lines it logged. */
  async function failedJob(
    error: DesktopJobErrorEvent,
  ): Promise<{ thrown: unknown; log: string[] }> {
    const log: string[] = [];
    const thrown = await awaitDesktopJob(
      "Import Run",
      async () => {
        queueMicrotask(() => listeners.get("desktop-job:error")?.({ payload: error }));
      },
      (line) => log.push(line),
    ).catch((e: unknown) => e);
    return { thrown, log };
  }

  // The desktop app decides a Cancel from the error's type, so a Cancel
  // whose text gained context on the way out is still a Cancel (#2077).
  it("rejects with a CanceledError when a Cancel stopped the job, whatever its text", async () => {
    const { thrown, log } = await failedJob({
      detail: "write a conversation: canceled",
      canceled: true,
    });
    expect(thrown).toBeInstanceOf(CanceledError);
    expect(log).toEqual([]);
  });

  it("rejects a failed job with a plain error, even when its text says canceled", async () => {
    const { thrown } = await failedJob({ detail: "canceled", canceled: false });
    expect(thrown).not.toBeInstanceOf(CanceledError);
    expect(thrown).toEqual(new Error("canceled"));
  });

  // A panic's text, or the path an operating system error names, reaches the
  // log and stays on the error, while the person reads the sentence (#2071, #2112).
  it("shows the sentence for the person and keeps the error chain in the log and the cause", async () => {
    const { thrown, log } = await failedJob({
      detail: "the job panicked: index out of bounds",
      user_message: "Staging failed because of a bug in Message Crate.",
      canceled: false,
    });
    expect(thrown).toBeInstanceOf(DesktopJobError);
    expect((thrown as Error).message).toBe("Staging failed because of a bug in Message Crate.");
    expect((thrown as DesktopJobError).detail).toBe("the job panicked: index out of bounds");
    expect(log).toEqual(["the job panicked: index out of bounds"]);
  });
});
