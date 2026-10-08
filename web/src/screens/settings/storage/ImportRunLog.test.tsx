/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installIntersectionObserver } from "../../../test/intersectionObserver";
import { Providers } from "../../../test/providers";
import { setupUser } from "../../../test/user";
import ImportRunLog from "./ImportRunLog";

const desktop = vi.hoisted(() => ({ on: true }));
const getAccountProfile = vi.hoisted(() => vi.fn());
const invokeListImportRunLogs = vi.hoisted(() => vi.fn());
const invokeReadImportRunLogLines = vi.hoisted(() => vi.fn());

vi.mock("../../../lib/auth", () => ({ useAuth: () => ({ accountId: 2 }) }));
vi.mock("../../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/api")>()),
  getBaseUrl: () => "http://127.0.0.1:8080",
}));
vi.mock("../../../lib/tauri-check", () => ({ isTauri: () => desktop.on }));
vi.mock("../../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/serverApi")>()),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
}));
vi.mock("../../../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/tauri")>()),
  invokeListImportRunLogs: (...a: unknown[]) => invokeListImportRunLogs(...a),
  invokeReadImportRunLogLines: (...a: unknown[]) => invokeReadImportRunLogLines(...a),
}));

const SERVER = "http://127.0.0.1:8080";

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
    invokeListImportRunLogs.mockResolvedValue([
      {
        name: "import-whatsapp-261004-143000.log",
        account: { importRunId: 42, accountId: 2, server: SERVER },
        bytes: 120,
        modifiedAt: "2026-10-04T14:35:00Z",
      },
      // The same run number on another server is another run.
      {
        name: "import-sms-261005-090000.log",
        account: { importRunId: 43, accountId: 2, server: "http://192.168.1.20:8080" },
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
    const reader = { server: SERVER, accountId: 2, owner: false };
    expect(invokeListImportRunLogs).toHaveBeenCalledWith(reader);
    expect(invokeReadImportRunLogLines).toHaveBeenCalledWith(
      reader,
      "import-whatsapp-261004-143000.log",
      expect.objectContaining({ level: "warn" }),
    );
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
