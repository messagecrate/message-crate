/** @vitest-environment jsdom */

import { cleanup, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../../test/providers";
import { setupUser } from "../../../test/user";
import { StorageSection } from "../StorageSection";

const getAccountProfile = vi.hoisted(() => vi.fn());
const getAccountStorage = vi.hoisted(() => vi.fn());
const listAccountImports = vi.hoisted(() => vi.fn());
const listAccountExports = vi.hoisted(() => vi.fn());
const getAccountImport = vi.hoisted(() => vi.fn());

vi.mock("../../../lib/auth", () => ({
  useAuth: () => ({ accountId: 7 }),
}));

vi.mock("../../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/serverApi")>()),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  getAccountStorage: (...a: unknown[]) => getAccountStorage(...a),
  listAccountImports: (...a: unknown[]) => listAccountImports(...a),
  listAccountExports: (...a: unknown[]) => listAccountExports(...a),
  getAccountImport: (...a: unknown[]) => getAccountImport(...a),
}));

/** One Import Run, told apart on screen by its source. */
function anImport(id: number) {
  return {
    id,
    source: `source-${id}`,
    status: "completed",
    started_at: "2024-01-01T00:00:00Z",
    finished_at: "2024-01-01T00:05:00Z",
    message_count: 10,
    attachment_count: 1,
    bytes_uploaded: 1024,
    issue_count: 0,
  };
}

describe("Import history", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getAccountProfile.mockResolvedValue({ account_id: 7, message_count: 0 });
    getAccountStorage.mockResolvedValue({
      total_bytes: 0,
      attachment_count: 0,
      conversation_count: 0,
      contact_count: 0,
      top_attachments: [],
    });
    listAccountExports.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    // 120 runs make three pages of 50. The server answers the page it is asked for.
    listAccountImports.mockImplementation(({ offset }: { offset: number }) =>
      Promise.resolve({ items: [anImport(offset + 1)], total: 120, limit: 50, offset }),
    );
  });

  afterEach(() => {
    cleanup();
  });

  it("asks the server for each page of 50 and stops at the last", async () => {
    const user = setupUser();
    renderWithProviders(<StorageSection />);

    const heading = await screen.findByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);
    const back = () => section.getByRole("button", { name: "Back" });
    const next = () => section.getByRole("button", { name: "Next" });

    expect(await section.findByText("Page 1 of 3")).toBeInTheDocument();
    expect(listAccountImports).toHaveBeenCalledWith(
      { limit: 50, offset: 0 },
      expect.anything(),
      undefined,
    );
    expect(back()).toBeDisabled();
    expect(next()).toBeEnabled();

    await user.click(next());
    expect(await section.findByText("Page 2 of 3")).toBeInTheDocument();
    expect(listAccountImports).toHaveBeenCalledWith(
      { limit: 50, offset: 50 },
      expect.anything(),
      undefined,
    );
    expect(await section.findByText("source-51")).toBeInTheDocument();
    expect(back()).toBeEnabled();

    await user.click(next());
    expect(await section.findByText("Page 3 of 3")).toBeInTheDocument();
    await waitFor(() => expect(next()).toBeDisabled());
  });

  it("says a run that has not finished has not finished, in place of a finish time", async () => {
    const running = {
      ...anImport(1),
      status: "running",
      stage: "staging_review",
      started_at: "2024-01-01T00:00:00Z",
      finished_at: null,
      summary: null,
      issues: [],
    };
    listAccountImports.mockResolvedValue({ items: [running], total: 1, limit: 50, offset: 0 });
    getAccountImport.mockResolvedValue(running);
    const user = setupUser();
    renderWithProviders(<StorageSection />);
    const heading = await screen.findByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);
    await user.click(await section.findByRole("button", { expanded: false }));
    const finished = await screen.findByText("Finished");
    const started = screen.getByText("Started");
    expect(finished.nextElementSibling?.textContent).not.toBe(
      started.nextElementSibling?.textContent,
    );
    expect(finished.nextElementSibling?.textContent).toBe("Not finished");
  });

  it("shows how many issues each run recorded, from the list's count", async () => {
    listAccountImports.mockResolvedValue({
      items: [{ ...anImport(1), issue_count: 20_000 }],
      total: 1,
      limit: 50,
      offset: 0,
    });
    renderWithProviders(<StorageSection />);
    const heading = await screen.findByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);

    const table = await section.findByRole("table");
    const headers = within(table)
      .getAllByRole("columnheader")
      .map((cell) => cell.textContent);
    const row = within(table).getAllByRole("row")[1];
    const cells = within(row)
      .getAllByRole("cell")
      .map((cell) => cell.textContent);
    expect(cells[headers.indexOf("Issues")]).toBe((20_000).toLocaleString());
    expect(getAccountImport).not.toHaveBeenCalled();
  });

  it("reads a run's issues when the run is opened", async () => {
    const listed = { ...anImport(1), status: "completed_with_issues", issue_count: 1 };
    listAccountImports.mockResolvedValue({ items: [listed], total: 1, limit: 50, offset: 0 });
    getAccountImport.mockResolvedValue({
      ...listed,
      summary: null,
      contacts_new: 0,
      contacts_changed: 0,
      issues: [{ kind: "skip", stage: "staging", item: "chat-1.txt", reason: "empty" }],
    });
    const user = setupUser();
    renderWithProviders(<StorageSection />);
    const heading = await screen.findByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);

    await user.click(await section.findByRole("button", { expanded: false }));

    expect(await screen.findByRole("heading", { name: "Import Errors" })).toBeInTheDocument();
    expect(getAccountImport).toHaveBeenCalledWith(1, expect.anything(), undefined);
  });
});
