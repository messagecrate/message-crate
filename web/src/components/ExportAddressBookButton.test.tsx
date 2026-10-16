/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { saveFile } from "../lib/saveFile";
import { exportAddressBook } from "../lib/serverApi";
import { settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { nth } from "../test/nth";
import ExportAddressBookButton from "./ExportAddressBookButton";

vi.mock("../lib/serverApi", () => ({ exportAddressBook: vi.fn() }));
vi.mock("../lib/saveFile", () => ({ saveFile: vi.fn() }));

const exportMock = vi.mocked(exportAddressBook);
const saveMock = vi.mocked(saveFile);

const CSV = "contact_id,display_name,service,identity,groups\n7,,Text Message,+15555550100,\n";

describe("ExportAddressBookButton", () => {
  beforeEach(() => {
    startFakeClock();
    vi.clearAllMocks();
    exportMock.mockResolvedValue(CSV);
    saveMock.mockResolvedValue(true);
  });

  afterEach(() => {
    cleanup();
    stopFakeClock();
  });

  it("exports the list's search when no row is checked, and saves what the server answers", async () => {
    render(<ExportAddressBookButton query="group:Unknown" checkedIds={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "Export" }));

    await settle();
    expect(saveMock).toHaveBeenCalledTimes(1);
    expect(exportMock).toHaveBeenCalledWith({ q: "group:Unknown" });
    const [name, file] = nth(saveMock.mock.calls, 0);
    expect(name).toBe("address-book.csv");
    expect(file.type).toBe("text/csv");
    expect(await file.text()).toBe(CSV);
  });

  it("exports the checked rows alone when there are any", async () => {
    render(<ExportAddressBookButton query="ada" checkedIds={[4, 9]} />);
    fireEvent.click(screen.getByRole("button", { name: "Export" }));

    await settle();
    expect(saveMock).toHaveBeenCalledTimes(1);
    expect(exportMock).toHaveBeenCalledWith({ ids: [4, 9] });
  });

  it("says why when the export fails, and saves nothing", async () => {
    exportMock.mockRejectedValue(new Error("the search has a word Contacts does not know"));
    render(<ExportAddressBookButton query="nosuchword:1" checkedIds={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "Export" }));

    await settle();
    expect(screen.getByRole("alert").textContent).toBe(
      "The search has a word Contacts does not know",
    );
    expect(saveMock).not.toHaveBeenCalled();
  });
});
