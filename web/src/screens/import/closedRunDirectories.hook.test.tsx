/** @vitest-environment jsdom */

// Opening Import in the desktop app deletes the directories of the account's
// closed Import Runs on the Message Crate it is signed in to (#2822).

import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invokeDeleteClosedRunDir, invokeListAccountRunDirs } from "../../lib/desktop/runDir";
import { getImport, getServerState } from "../../lib/serverApi";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { authModule, Providers } from "../../test/providers";
import { useDeleteClosedRunDirectories } from "./closedRunDirectories";

const desktop = vi.hoisted(() => ({ isTauri: true }));

vi.mock("../../lib/authContext", () => authModule({ accountId: 7 }));
vi.mock("../../lib/tauriCheck", () => ({ isTauri: () => desktop.isTauri }));
vi.mock("../../lib/desktop/runDir", () => ({
  invokeListAccountRunDirs: vi.fn(),
  invokeDeleteClosedRunDir: vi.fn(),
}));
vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  getImport: vi.fn(),
  getServerState: vi.fn(),
}));

const list = vi.mocked(invokeListAccountRunDirs);
const deleteClosed = vi.mocked(invokeDeleteClosedRunDir);
const readRun = vi.mocked(getImport);
const readServer = vi.mocked(getServerState);

const closedRun = { runDir: "/home/sam/message-crate/staging/a", importRunId: 41 };
const runningRun = { runDir: "/home/sam/message-crate/staging/b", importRunId: 42 };

/** The server answers each run's status from `statuses`. */
function serverReports(statuses: Record<number, string>) {
  readRun.mockImplementation(async (id: number) => {
    // The hook reads `status` alone.
    return { id, status: statuses[id] } as unknown as Awaited<ReturnType<typeof getImport>>;
  });
}

async function openImport() {
  renderHook(() => useDeleteClosedRunDirectories(), { wrapper: Providers });
  await settle();
}

beforeEach(() => {
  startFakeClock();
  desktop.isTauri = true;
  readServer.mockResolvedValue({ id: "3f2a9c" } as Awaited<ReturnType<typeof getServerState>>);
  list.mockResolvedValue([closedRun, runningRun]);
  deleteClosed.mockResolvedValue(true);
});

afterEach(() => {
  stopFakeClock();
  vi.clearAllMocks();
});

describe("opening Import", () => {
  it("deletes a closed run's directory and keeps a running run's", async () => {
    serverReports({ 41: "canceled", 42: "running" });

    await openImport();

    expect(list).toHaveBeenCalledWith(7, "3f2a9c");
    expect(deleteClosed.mock.calls).toEqual([
      [closedRun, { accountId: 7, messageCrateId: "3f2a9c" }, "canceled"],
    ]);
  });

  it("deletes nothing when the server does not answer", async () => {
    readRun.mockRejectedValue(new TypeError("Failed to fetch"));

    await openImport();

    expect(deleteClosed).not.toHaveBeenCalled();
  });

  it("looks at no directory in a browser", async () => {
    desktop.isTauri = false;
    serverReports({ 41: "canceled", 42: "running" });

    await openImport();

    expect(list).not.toHaveBeenCalled();
    expect(deleteClosed).not.toHaveBeenCalled();
  });
});
