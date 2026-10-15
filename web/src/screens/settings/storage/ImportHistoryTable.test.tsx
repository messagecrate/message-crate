/** @vitest-environment jsdom */

import { cleanup, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { settle, startFakeClock, stopFakeClock } from "../../../test/fakeTimers";
import { nth } from "../../../test/nth";
import { renderWithProviders } from "../../../test/providers";
import { setupUser } from "../../../test/user";
import { StorageSection } from "../StorageSection";

const getAccountProfile = vi.hoisted(() => vi.fn());
const getAccountStorage = vi.hoisted(() => vi.fn());
const listAccountImports = vi.hoisted(() => vi.fn());
const listAccountExports = vi.hoisted(() => vi.fn());
const getAccountImport = vi.hoisted(() => vi.fn());

vi.mock("../../../lib/authContext", () => ({
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
    listAccountExports.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    // 120 runs make three pages of 50. The server answers the page it is asked for.
    listAccountImports.mockImplementation(({ offset }: { offset: number }) =>
      Promise.resolve({ items: [anImport(offset + 1)], total: 120, limit: 50, offset }),
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

    const heading = screen.getByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);
    const back = () => section.getByRole("button", { name: "Back" });
    const next = () => section.getByRole("button", { name: "Next" });

    expect(section.getByText("Page 1 of 3")).toBeInTheDocument();
    expect(listAccountImports).toHaveBeenCalledWith(
      { limit: 50, offset: 0 },
      expect.anything(),
      undefined,
    );
    expect(back()).toBeDisabled();
    expect(next()).toBeEnabled();

    await user.click(next());
    await settle();
    expect(section.getByText("Page 2 of 3")).toBeInTheDocument();
    expect(listAccountImports).toHaveBeenCalledWith(
      { limit: 50, offset: 50 },
      expect.anything(),
      undefined,
    );
    expect(section.getByText("source-51")).toBeInTheDocument();
    expect(back()).toBeEnabled();

    await user.click(next());
    await settle();
    expect(section.getByText("Page 3 of 3")).toBeInTheDocument();
    expect(section.getByText("source-101")).toBeInTheDocument();
    expect(next()).toBeDisabled();
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
    await settle();
    const heading = screen.getByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);
    await user.click(section.getByRole("button", { expanded: false }));
    await settle();
    const finished = screen.getByText("Finished");
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
    await settle();
    const heading = screen.getByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);

    const table = section.getByRole("table");
    const headers = within(table)
      .getAllByRole("columnheader")
      .map((cell) => cell.textContent);
    const row = nth(within(table).getAllByRole("row"), 1);
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
    await settle();
    const heading = screen.getByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);

    await user.click(section.getByRole("button", { expanded: false }));
    await settle();

    expect(screen.getByRole("heading", { name: "Import Errors" })).toBeInTheDocument();
    expect(getAccountImport).toHaveBeenCalledWith(1, expect.anything(), undefined);
    // The account's own run carries no list of missing versions: the account
    // reads why on each attachment it opens.
    expect(screen.queryByRole("heading", { name: "Thumbnails and Previews" })).toBeNull();
  });

  it("shows the backup a run read beside when that backup was made", async () => {
    const listed = { ...anImport(1) };
    listAccountImports.mockResolvedValue({ items: [listed], total: 1, limit: 50, offset: 0 });
    getAccountImport.mockResolvedValue({
      ...listed,
      summary: null,
      contacts_new: 0,
      contacts_changed: 0,
      issues: [],
      source_fingerprint: { path: "/backups/iPhone/00008110", size: 12, mtime_ms: 1 },
      backup_taken_at: "2026-09-30T18:45:12.000Z",
    });
    const user = setupUser();
    renderWithProviders(<StorageSection />);
    await settle();
    const heading = screen.getByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);
    await user.click(section.getByRole("button", { expanded: false }));
    await settle();

    const backup = screen.getByText("Backup");
    const [file, made] = Array.from(backup.parentElement?.querySelectorAll("dd") ?? []);
    expect(file?.textContent).toBe("/backups/iPhone/00008110");
    expect(made?.textContent).toMatch(/^Made .*2026/);
  });

  it("says when a run's backup does not say when it was made", async () => {
    const listed = { ...anImport(1) };
    listAccountImports.mockResolvedValue({ items: [listed], total: 1, limit: 50, offset: 0 });
    getAccountImport.mockResolvedValue({
      ...listed,
      summary: null,
      contacts_new: 0,
      contacts_changed: 0,
      issues: [],
      source_fingerprint: null,
      backup_taken_at: null,
    });
    const user = setupUser();
    renderWithProviders(<StorageSection />);
    await settle();
    const heading = screen.getByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);
    await user.click(section.getByRole("button", { expanded: false }));
    await settle();

    const backup = screen.getByText("Backup");
    const [file, made] = Array.from(backup.parentElement?.querySelectorAll("dd") ?? []);
    expect(file?.textContent).toBe("Not recorded");
    expect(made?.textContent).toBe("The backup does not say when it was made");
  });

  it("shows the owner which attachments of a run have no Thumbnail or Preview, and why", async () => {
    const listed = {
      ...anImport(1),
      counts: {},
      contacts_new: 0,
      contacts_changed: 0,
    };
    listAccountImports.mockResolvedValue({ items: [listed], total: 1, limit: 50, offset: 0 });
    getAccountImport.mockResolvedValue({
      ...listed,
      written_attachment_count: 5,
      not_made_counts: [
        { kind: "conversion_failed", thumbnails: 1, previews: 1 },
        { kind: "stalled", thumbnails: 0, previews: 2 },
      ],
      not_made_attachments: [
        {
          id: 11,
          original_name: "IMG_0001.HEIC",
          mime_type: "image/heic",
          size_bytes: 2048,
          thumbnail: { kind: "conversion_failed", reason: "Invalid data found" },
          preview: { kind: "conversion_failed", reason: "Invalid data found" },
        },
        {
          id: 12,
          original_name: "clip.mov",
          mime_type: "video/quicktime",
          size_bytes: null,
          thumbnail: null,
          preview: { kind: "stalled", reason: "ffmpeg made no progress for 2 minutes" },
        },
      ],
    });
    const user = setupUser();
    renderWithProviders(<StorageSection managedAccountId={3} />);
    await settle();
    const heading = screen.getByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);
    await user.click(section.getByRole("button", { expanded: false }));
    await settle();

    const panel = within(
      screen.getByRole("heading", { name: "Thumbnails and Previews" }).parentElement as HTMLElement,
    );
    // The failures are out of the run's attachments, 5 here.
    expect(panel.getByText("2 of 5 attachments have no Thumbnail or Preview")).toBeInTheDocument();
    const [counts, list] = panel.getAllByRole("table");
    const countRows = within(counts as HTMLElement)
      .getAllByRole("row")
      .slice(1)
      .map((row) =>
        within(row)
          .getAllByRole("cell")
          .map((cell) => cell.textContent),
      );
    expect(countRows).toEqual([
      ["Conversion failed", "1", "1"],
      ["Stopped making progress", "0", "2"],
    ]);
    const clip = within(list as HTMLElement)
      .getByText("clip.mov")
      .closest("tr") as HTMLElement;
    const cells = within(clip)
      .getAllByRole("cell")
      .map((cell) => cell.textContent);
    expect(cells).toEqual([
      "clip.mov",
      "video/quicktime",
      "—",
      "—",
      "Stopped making progressffmpeg made no progress for 2 minutes",
    ]);
    expect(getAccountImport).toHaveBeenCalledWith(1, expect.anything(), 3);
  });

  it("tells the owner when no attachment of a run is missing a version", async () => {
    const listed = { ...anImport(1), counts: {}, contacts_new: 0, contacts_changed: 0 };
    listAccountImports.mockResolvedValue({ items: [listed], total: 1, limit: 50, offset: 0 });
    getAccountImport.mockResolvedValue({
      ...listed,
      written_attachment_count: 1,
      not_made_counts: [],
      not_made_attachments: [],
    });
    const user = setupUser();
    renderWithProviders(<StorageSection managedAccountId={3} />);
    await settle();
    const heading = screen.getByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);
    await user.click(section.getByRole("button", { expanded: false }));
    await settle();

    expect(
      screen.getByText(
        "No attachment of this import is missing a Thumbnail or Preview the server tried to make.",
      ),
    ).toBeInTheDocument();
  });

  it("says when no Import Run has been recorded", async () => {
    listAccountImports.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    renderWithProviders(<StorageSection />);
    await settle();

    const heading = screen.getByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);
    expect(section.getByText("No imports recorded yet")).toBeInTheDocument();
  });
});
