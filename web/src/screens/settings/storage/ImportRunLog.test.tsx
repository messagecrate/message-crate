/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { settle, startFakeClock, stopFakeClock } from "../../../test/fakeTimers";
import { installIntersectionObserver } from "../../../test/intersectionObserver";
import { authModule, Providers } from "../../../test/providers";
import { setupUser } from "../../../test/user";
import ImportRunLog from "./ImportRunLog";

const desktop = vi.hoisted(() => ({ on: true }));
const getAccountProfile = vi.hoisted(() => vi.fn());
const getServerState = vi.hoisted(() => vi.fn());
const invokeListLogs = vi.hoisted(() => vi.fn());
const invokeReadLogLines = vi.hoisted(() => vi.fn());

vi.mock("../../../hooks/useAuth", () => authModule({ accountId: 2 }));
vi.mock("../../../lib/tauriCheck", () => ({ isTauri: () => desktop.on }));
vi.mock("../../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/serverApi")>()),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  getServerState: (...a: unknown[]) => getServerState(...a),
}));
vi.mock("../../../lib/desktop/runLog", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/desktop/runLog")>()),
  invokeListLogs: (...a: unknown[]) => invokeListLogs(...a),
  invokeReadLogLines: (...a: unknown[]) => invokeReadLogLines(...a),
}));

const SERVER = "http://127.0.0.1:8080";
/** The id of the Message Crate the window is signed in to. */
const HERE = "0123456789abcdef0123456789abcdef";

function renderRow(importRunId: number) {
  render(
    <Providers>
      <ImportRunLog importRunId={importRunId} />
    </Providers>,
  );
}

describe("ImportRunLog", () => {
  beforeEach(() => {
    startFakeClock();
    vi.clearAllMocks();
    installIntersectionObserver();
    desktop.on = true;
    getAccountProfile.mockResolvedValue({ is_admin: false });
    getServerState.mockResolvedValue({ id: HERE });
    invokeListLogs.mockResolvedValue([
      {
        name: "import-whatsapp-261004-143000.log",
        kind: "importRun",
        account: { importRunId: 42, accountId: 2, server: SERVER, messageCrateId: HERE },
        thisMessageCrate: true,
        hasLines: true,
        bytes: 120,
        modifiedAt: "2026-10-04T14:35:00Z",
      },
      // Run 43 of another Message Crate at the same address is another run.
      {
        name: "import-sms-261005-090000.log",
        kind: "importRun",
        account: {
          importRunId: 43,
          accountId: 2,
          server: SERVER,
          messageCrateId: "fedcba9876543210fedcba9876543210",
        },
        thisMessageCrate: false,
        hasLines: true,
        bytes: 80,
        modifiedAt: "2026-10-05T09:05:00Z",
      },
    ]);
    invokeReadLogLines.mockResolvedValue({
      items: [
        { id: 0, time: "2026-10-04T14:30:00.000000Z", level: "warn", text: "a.jpg: missing" },
      ],
      limit: 200,
      has_more: false,
    });
  });

  afterEach(() => {
    cleanup();
    stopFakeClock();
  });

  it("opens the run's log from its row when the log is on this computer", async () => {
    const user = setupUser();
    renderRow(42);
    await settle();

    await user.click(screen.getByRole("button", { name: "Open this run's log" }));
    await settle();

    expect(screen.getByText("a.jpg: missing")).toBeInTheDocument();
    const reader = { messageCrateId: HERE, accountId: 2, admin: false };
    expect(invokeListLogs).toHaveBeenCalledWith(reader);
    expect(invokeReadLogLines).toHaveBeenCalledWith(
      reader,
      "import-whatsapp-261004-143000.log",
      expect.objectContaining({ level: "warn" }),
    );
  });

  it("says a log written before its lines had a level cannot be shown, and still downloads it", async () => {
    invokeListLogs.mockResolvedValue([
      {
        name: "import-whatsapp-261004-143000.log",
        kind: "importRun",
        account: { importRunId: 42, accountId: 2, server: SERVER, messageCrateId: HERE },
        thisMessageCrate: true,
        hasLines: false,
        bytes: 120,
        modifiedAt: "2026-10-04T14:35:00Z",
      },
    ]);
    const user = setupUser();
    renderRow(42);
    await settle();

    await user.click(screen.getByRole("button", { name: "Open this run's log" }));
    await settle();

    expect(
      screen.getByText(/written before its lines carried a time and a level/),
    ).toBeInTheDocument();
    expect(screen.queryByText("No line at this level")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
  });

  it("offers nothing for a run whose log is not on this computer", async () => {
    renderRow(43);

    await settle();
    expect(invokeListLogs).toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Open this run's log" })).not.toBeInTheDocument();
  });

  it("offers nothing in a browser", () => {
    desktop.on = false;
    renderRow(42);

    expect(screen.queryByRole("button", { name: "Open this run's log" })).not.toBeInTheDocument();
    expect(invokeListLogs).not.toHaveBeenCalled();
  });
});
