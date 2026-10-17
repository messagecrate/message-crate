/** @vitest-environment jsdom */

import { cleanup, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { settle, startFakeClock, stopFakeClock } from "../../../test/fakeTimers";
import { authModule, renderWithProviders } from "../../../test/providers";
import { setupUser } from "../../../test/user";
import { StorageSection } from "../StorageSection";

const getAccountProfile = vi.hoisted(() => vi.fn());
const getAccountStorage = vi.hoisted(() => vi.fn());
const listAccountImports = vi.hoisted(() => vi.fn());
const listAccountExports = vi.hoisted(() => vi.fn());

vi.mock("../../../hooks/useAuth", () => authModule({ accountId: 7 }));

vi.mock("../../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/serverApi")>()),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  getAccountStorage: (...a: unknown[]) => getAccountStorage(...a),
  listAccountImports: (...a: unknown[]) => listAccountImports(...a),
  listAccountExports: (...a: unknown[]) => listAccountExports(...a),
}));

/** One Export Run, told apart on screen by the search it ran. */
function anExport(id: number) {
  return {
    id,
    scope: { kind: "query", list: "messages", q: `run-${id}` },
    status: "completed",
    started_at: "2024-01-01T00:00:00Z",
    finished_at: "2024-01-01T00:05:00Z",
    message_count: 10,
    messages_delivered: 10,
    conversation_count: 1,
    attachment_count: 1,
    total_bytes: 1024,
  };
}

describe("Export history", () => {
  beforeEach(() => {
    startFakeClock();
    vi.clearAllMocks();
    getAccountProfile.mockResolvedValue({ account_id: 7, message_count: 0 });
    getAccountStorage.mockResolvedValue({
      total_bytes: 0,
      attachment_count: 0,
      conversation_count: 0,
      contact_count: 0,
      top_attachments: [],
    });
    listAccountImports.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    // 120 runs make three pages of 50. The server answers the page it is asked for.
    listAccountExports.mockImplementation(({ offset }: { offset: number }) =>
      Promise.resolve({ items: [anExport(offset + 1)], total: 120, limit: 50, offset }),
    );
  });

  afterEach(() => {
    cleanup();
    stopFakeClock();
  });

  it("asks the server for each page of 50 and stops at the last", async () => {
    const user = setupUser();
    renderWithProviders(<StorageSection />);
    await settle();

    const heading = screen.getByRole("heading", { name: "Export history" });
    const section = within(heading.parentElement as HTMLElement);
    const back = () => section.getByRole("button", { name: "Back" });
    const next = () => section.getByRole("button", { name: "Next" });

    expect(section.getByText("Page 1 of 3")).toBeInTheDocument();
    expect(listAccountExports).toHaveBeenCalledWith(
      { limit: 50, offset: 0 },
      expect.anything(),
      undefined,
    );
    expect(back()).toBeDisabled();
    expect(next()).toBeEnabled();

    await user.click(next());
    await settle();
    expect(section.getByText("Page 2 of 3")).toBeInTheDocument();
    expect(listAccountExports).toHaveBeenCalledWith(
      { limit: 50, offset: 50 },
      expect.anything(),
      undefined,
    );
    expect(section.getByText("Messages found by: run-51")).toBeInTheDocument();
    expect(back()).toBeEnabled();

    await user.click(next());
    await settle();
    expect(section.getByText("Page 3 of 3")).toBeInTheDocument();
    expect(section.getByText("Messages found by: run-101")).toBeInTheDocument();
    expect(next()).toBeDisabled();
  });

  it("says when no Export Run has been recorded", async () => {
    listAccountExports.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    renderWithProviders(<StorageSection />);
    await settle();

    const heading = screen.getByRole("heading", { name: "Export history" });
    const section = within(heading.parentElement as HTMLElement);
    expect(section.getByText("No exports recorded yet")).toBeInTheDocument();
  });
});
