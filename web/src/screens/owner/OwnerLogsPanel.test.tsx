/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installIntersectionObserver } from "../../test/intersectionObserver";
import { Providers } from "../../test/providers";
import { setupUser } from "../../test/user";
import { OwnerLogsPanel } from "./OwnerLogsPanel";

const desktop = vi.hoisted(() => ({ on: true }));
const listServerLogLines = vi.hoisted(() => vi.fn());
const listServerLogFiles = vi.hoisted(() => vi.fn());
const listAccounts = vi.hoisted(() => vi.fn());
const getAccountProfile = vi.hoisted(() => vi.fn());
const getServerState = vi.hoisted(() => vi.fn());
const invokeListImportRunLogs = vi.hoisted(() => vi.fn());
const invokeReadImportRunLogLines = vi.hoisted(() => vi.fn());

vi.mock("../../lib/auth", () => ({ useAuth: () => ({ accountId: 1 }) }));
vi.mock("../../lib/tauri-check", () => ({ isTauri: () => desktop.on }));
vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  listServerLogLines: (...a: unknown[]) => listServerLogLines(...a),
  listServerLogFiles: (...a: unknown[]) => listServerLogFiles(...a),
  listAccounts: (...a: unknown[]) => listAccounts(...a),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  getServerState: (...a: unknown[]) => getServerState(...a),
}));
vi.mock("../../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/tauri")>()),
  invokeListImportRunLogs: (...a: unknown[]) => invokeListImportRunLogs(...a),
  invokeReadImportRunLogLines: (...a: unknown[]) => invokeReadImportRunLogLines(...a),
}));

const page = (text: string) => ({
  items: [{ id: 1, time: "2026-10-08T12:00:00.000000Z", level: "warn", text }],
  limit: 200,
  has_more: false,
});

/** The id of the Message Crate the window is signed in to. */
const HERE = "0123456789abcdef0123456789abcdef";

const ALICE_RUN = {
  name: "import-whatsapp-261004-143000.log",
  account: { importRunId: 42, accountId: 2, server: "http://127.0.0.1:8080", messageCrateId: HERE },
  thisMessageCrate: true,
  hasLines: true,
  bytes: 120,
  modifiedAt: "2026-10-04T14:35:00Z",
};

function renderPanel() {
  render(
    <Providers>
      <OwnerLogsPanel />
    </Providers>,
  );
}

describe("OwnerLogsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    installIntersectionObserver();
    desktop.on = true;
    getAccountProfile.mockResolvedValue({ is_owner: true });
    getServerState.mockResolvedValue({ id: HERE });
    listAccounts.mockResolvedValue([
      { account_id: 1, username: "root" },
      { account_id: 2, username: "alice" },
    ]);
    listServerLogLines.mockResolvedValue(page("the server refused a login"));
    listServerLogFiles.mockResolvedValue([
      { id: 2, name: "server-000002.log", bytes: 10, modified_at: "2026-10-08T12:00:00Z" },
      { id: 1, name: "server-000001.log", bytes: 50, modified_at: "2026-10-07T12:00:00Z" },
    ]);
    invokeListImportRunLogs.mockResolvedValue([ALICE_RUN]);
    invokeReadImportRunLogLines.mockResolvedValue(page("chat.jsonl failed: the server refused"));
  });

  afterEach(() => {
    cleanup();
  });

  it("opens on the server's log, and picks an Import Run's log on this computer", async () => {
    const user = setupUser();
    renderPanel();

    expect(await screen.findByText("the server refused a login")).toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: "Download server-000002.log" }),
    ).toBeInTheDocument();
    // The owner reads every run log on this computer.
    await waitFor(() =>
      expect(invokeListImportRunLogs).toHaveBeenCalledWith({
        messageCrateId: HERE,
        accountId: 1,
        owner: true,
      }),
    );

    await user.click(screen.getByRole("button", { name: /Log$/ }));
    await user.click(await screen.findByRole("option", { name: /Import Run 42 by alice/ }));

    expect(await screen.findByText("chat.jsonl failed: the server refused")).toBeInTheDocument();
    expect(invokeReadImportRunLogLines).toHaveBeenCalledWith(
      { messageCrateId: HERE, accountId: 1, owner: true },
      ALICE_RUN.name,
      expect.objectContaining({ level: "warn" }),
    );
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
  });

  it("offers the server's log alone in a browser", async () => {
    desktop.on = false;
    const user = setupUser();
    renderPanel();

    expect(await screen.findByText("the server refused a login")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Log$/ }));
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual(["The server's log"]);
    expect(invokeListImportRunLogs).not.toHaveBeenCalled();
  });
});
