import { describe, expect, it, vi } from "vitest";
import { ApiError } from "../../lib/api";
import type { ClosedImportStatus, RunDirOfImportRun } from "../../lib/desktop/runDir";
import type { ImportStatus } from "../../lib/importRun";
import { type ClosedRunDirectories, deleteClosedRunDirectories } from "./closedRunDirectories";

const closed: RunDirOfImportRun = { runDir: "/home/sam/message-crate/staging/a", importRunId: 41 };
const running: RunDirOfImportRun = { runDir: "/home/sam/message-crate/staging/b", importRunId: 42 };

/** Calls whose server answers each run's status from `statuses`, refusing a run it has none for. */
function calls(
  runs: RunDirOfImportRun[],
  statuses: Record<number, ImportStatus>,
): ClosedRunDirectories & { deleted: [string, ClosedImportStatus][] } {
  const deleted: [string, ClosedImportStatus][] = [];
  return {
    deleted,
    list: async () => runs,
    status: async (id) => {
      const status = statuses[id];
      if (!status) throw new ApiError(404, "Not Found");
      return status;
    },
    deleteClosed: async (run, status) => {
      deleted.push([run.runDir, status]);
      return true;
    },
  };
}

describe("deleting the directories of closed Import Runs when Import opens (#2822)", () => {
  it("deletes a closed run's directory and keeps a running run's", async () => {
    const c = calls([closed, running], { 41: "cancelled", 42: "running" });

    await deleteClosedRunDirectories(c);

    expect(c.deleted).toEqual([[closed.runDir, "cancelled"]]);
  });

  it("deletes the directory of every way a run closes", async () => {
    const runs = [41, 43, 44, 45].map((importRunId) => ({
      runDir: `/r/${importRunId}`,
      importRunId,
    }));
    const c = calls(runs, {
      41: "completed",
      43: "completed_with_issues",
      44: "failed",
      45: "cancelled",
    });

    await deleteClosedRunDirectories(c);

    expect(c.deleted.map(([, status]) => status)).toEqual([
      "completed",
      "completed_with_issues",
      "failed",
      "cancelled",
    ]);
  });

  it("deletes nothing when the server does not answer", async () => {
    const c = calls([closed, running], {});
    c.status = () => Promise.reject(new TypeError("Failed to fetch"));

    await deleteClosedRunDirectories(c);

    expect(c.deleted).toEqual([]);
  });

  it("keeps the directory of a run the server refuses to report", async () => {
    const c = calls([closed, running], { 42: "completed" });

    await deleteClosedRunDirectories(c);

    expect(c.deleted).toEqual([[running.runDir, "completed"]]);
  });

  it("goes on to the next run when one directory cannot be deleted", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const c = calls([closed, running], { 41: "failed", 42: "failed" });
    const deleteClosed = c.deleteClosed;
    c.deleteClosed = (run, status) =>
      run === closed ? Promise.reject(new Error("busy")) : deleteClosed(run, status);

    await deleteClosedRunDirectories(c);

    expect(c.deleted).toEqual([[running.runDir, "failed"]]);
    warn.mockRestore();
  });

  it("deletes nothing once the screen no longer wants the work", async () => {
    const c = calls([closed], { 41: "cancelled" });

    await deleteClosedRunDirectories(c, () => false);

    expect(c.deleted).toEqual([]);
  });
});
