/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installIntersectionObserver } from "../../../test/intersectionObserver";
import { Providers } from "../../../test/providers";
import { setupUser } from "../../../test/user";
import ImportRunLog from "./ImportRunLog";

const desktop = vi.hoisted(() => ({ on: true }));
const getAccountProfile = vi.hoisted(() => vi.fn());
const getServerState = vi.hoisted(() => vi.fn());
const invokeListImportRunLogs = vi.hoisted(() => vi.fn());
const invokeReadImportRunLogLines = vi.hoisted(() => vi.fn());

vi.mock("../../../lib/auth", () => ({ useAuth: () => ({ accountId: 2 }) }));
vi.mock("../../../lib/tauri-check", () => ({ isTauri: () => desktop.on }));
vi.mock("../../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/serverApi")>()),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  getServerState: (...a: unknown[]) => getServerState(...a),
}));
vi.mock("../../../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/tauri")>()),
  invokeListImportRunLogs: (...a: unknown[]) => invokeListImportRunLogs(...a),
  invokeReadImportRunLogLines: (...a: unknown[]) => invokeReadImportRunLogLines(...a),
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
    vi.clearAllMocks();
    installIntersectionObserver();
    desktop.on = true;
    getAccountProfile.mockResolvedValue({ is_owner: false });
    getServerState.mockResolvedValue({ id: HERE });
    invokeListImportRunLogs.mockResolvedValue([
      {
        name: "import-whatsapp-261004-143000.log",
        account: { importRunId: 42, accountId: 2, server: SERVER, messageCrateId: HERE },
        thisMessageCrate: true,
        hasLines: true,
        bytes: 120,
        modifiedAt: "2026-10-04T14:35:00Z",
      },
      // Run 43 of another Message Crate at the same address is another run.
      {
        name: "import-sms-261005-090000.log",
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
    invokeReadImportRunLogLines.mockResolvedValue({
      items: [
        { id: 0, time: "2026-10-04T14:30:00.000000Z", level: "warn", text: "a.jpg: missing" },
      ],
      limit: 200,
      has_more: false,
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("opens the run's log from its row when the log is on this computer", async () => {
    const user = setupUser();
    renderRow(42);

    await user.click(await screen.findByRole("button", { name: "Open this run's log" }));

    expect(await screen.findByText("a.jpg: missing")).toBeInTheDocument();
    const reader = { messageCrateId: HERE, accountId: 2, owner: false };
    expect(invokeListImportRunLogs).toHaveBeenCalledWith(reader);
    expect(invokeReadImportRunLogLines).toHaveBeenCalledWith(
      reader,
      "import-whatsapp-261004-143000.log",
      expect.objectContaining({ level: "warn" }),
    );
  });

  it("says a log written before its lines had a level cannot be shown, and still downloads it", async () => {
    invokeListImportRunLogs.mockResolvedValue([
      {
        name: "import-whatsapp-261004-143000.log",
        account: { importRunId: 42, accountId: 2, server: SERVER, messageCrateId: HERE },
        thisMessageCrate: true,
        hasLines: false,
        bytes: 120,
        modifiedAt: "2026-10-04T14:35:00Z",
      },
    ]);
    const user = setupUser();
    renderRow(42);

    await user.click(await screen.findByRole("button", { name: "Open this run's log" }));

    expect(
      await screen.findByText(/written before its lines carried a time and a level/),
    ).toBeInTheDocument();
    expect(screen.queryByText("No line at this level")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
  });

  it("offers nothing for a run whose log is not on this computer", async () => {
    renderRow(43);

    await waitFor(() => expect(invokeListImportRunLogs).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "Open this run's log" })).not.toBeInTheDocument();
  });

  it("offers nothing in a browser", () => {
    desktop.on = false;
    renderRow(42);

    expect(screen.queryByRole("button", { name: "Open this run's log" })).not.toBeInTheDocument();
    expect(invokeListImportRunLogs).not.toHaveBeenCalled();
  });
});
