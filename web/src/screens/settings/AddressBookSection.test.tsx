/** @vitest-environment jsdom */

import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../../lib/api";
import { keys } from "../../lib/queryKeys";
import { loadAddressBook } from "../../lib/serverApi";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { renderWithProviders, testQueryClient } from "../../test/providers";
import { freshEntries, seedEntries } from "../../test/staleEntries";
import { MIB } from "../../test/units";
import { setupUser } from "../../test/user";
import { AddressBookSection } from "./AddressBookSection";

vi.mock("../../lib/serverApi", () => ({ loadAddressBook: vi.fn() }));

vi.mock("../../lib/authContext", () => ({
  useAuth: () => ({ accountId: 7 }),
}));

const post = vi.mocked(loadAddressBook);

const FILE = "contact_id,display_name,groups,service,identity_type,identity\n";

const NOTHING = {
  contacts_created: 0,
  contacts_updated: 0,
  contacts_deleted: 0,
  identities_added: 0,
  identities_moved: 0,
  identities_removed: 0,
  groups_created: 0,
  notes: [],
};

function chooseFile(name: string, body: string) {
  const input = screen.getByLabelText("Address book file") as HTMLInputElement;
  const file = new File([body], name, { type: "text/plain" });
  fireEvent.change(input, { target: { files: [file] } });
  return file;
}

describe("AddressBookSection", () => {
  beforeEach(() => {
    startFakeClock();
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
    stopFakeClock();
  });

  it("offers Append and Edit, with Append chosen", () => {
    renderWithProviders(<AddressBookSection />);
    expect((screen.getByLabelText(/^Append/) as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText(/^Edit/) as HTMLInputElement).checked).toBe(false);
  });

  it("is one radio group, named and moved through with the arrow keys", async () => {
    renderWithProviders(<AddressBookSection />);
    const group = screen.getByRole("radiogroup", { name: "How to load it" });

    const user = setupUser();
    await user.tab();
    expect(within(group).getByRole("radio", { name: /^Append/ })).toHaveFocus();
    await user.keyboard("{ArrowDown}");

    expect(within(group).getByRole("radio", { name: /^Edit/ })).toBeChecked();
    expect(within(group).getByRole("radio", { name: /^Append/ })).not.toBeChecked();
  });

  it("sends the file's text as an Append unless Edit is chosen", async () => {
    post.mockResolvedValue(NOTHING);
    renderWithProviders(<AddressBookSection />);
    chooseFile("address-book.csv", FILE);
    await settle();
    expect(post).toHaveBeenCalledTimes(1);
    expect(post).toHaveBeenLastCalledWith(FILE, "append");

    fireEvent.click(screen.getByLabelText(/^Edit/));
    chooseFile("address-book.csv", FILE);
    await settle();
    expect(post).toHaveBeenCalledTimes(2);
    expect(post).toHaveBeenLastCalledWith(FILE, "edit");
  });

  it("takes a .csv file only, and refuses a vCard without asking the server", async () => {
    renderWithProviders(<AddressBookSection />);
    expect(screen.getByLabelText("Address book file").getAttribute("accept")).toBe(".csv,text/csv");
    chooseFile("Contacts.vcf", "BEGIN:VCARD\nEND:VCARD\n");

    await settle();
    expect(screen.getByText(/Choose a \.csv file/)).toBeInTheDocument();
    expect(post).not.toHaveBeenCalled();
  });

  it("shows the seven counts of what the load changed", async () => {
    post.mockResolvedValue({
      contacts_created: 1,
      contacts_updated: 2,
      contacts_deleted: 3,
      identities_added: 4,
      identities_moved: 5,
      identities_removed: 6,
      groups_created: 7,
      notes: [],
    });
    renderWithProviders(<AddressBookSection />);
    chooseFile("address-book.csv", FILE);

    await settle();
    const counts = screen.getByLabelText("What the load changed");
    const rows = within(counts)
      .getAllByRole("term")
      .map((term) => `${term.textContent}: ${term.nextElementSibling?.textContent}`);
    expect(rows).toEqual([
      "Contacts created: 1",
      "Contacts updated: 2",
      "Contacts deleted: 3",
      "Identities added: 4",
      "Identities moved: 5",
      "Identities removed: 6",
      "Contact Groups created: 7",
    ]);
  });

  it("lists each number the load read with its + back or made a new identity", async () => {
    // +65 5555 0100 is no one's number. The note on the `phone` crate's `mod
    // tests` says why.
    const notes = [
      'row 2: 6555550100 has no +, so it was read as +6555550100, which "Ada" (contact 4) holds',
      "row 5: 447700900123 has no +, so it became the new identity 447700900123",
    ];
    post.mockResolvedValue({ ...NOTHING, notes });
    renderWithProviders(<AddressBookSection />);
    chooseFile("address-book.csv", FILE);

    await settle();
    const list = screen.getByLabelText("Numbers written without +");
    expect(
      within(list)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual(notes);
  });

  it("shows no list of numbers when every number kept its +", async () => {
    post.mockResolvedValue(NOTHING);
    renderWithProviders(<AddressBookSection />);
    chooseFile("address-book.csv", FILE);

    await settle();
    expect(screen.getByLabelText("What the load changed")).toBeInTheDocument();
    expect(screen.queryByLabelText("Numbers written without +")).toBeNull();
  });

  it("marks the contacts and Contact Groups stale after a load", async () => {
    // A load renames contacts, moves identities and adds Contact Groups.
    const shown = [keys.contacts.all, keys.contactGroups.all];
    const client = testQueryClient();
    seedEntries(client, 7, shown);
    post.mockResolvedValue(NOTHING);
    render(
      <QueryClientProvider client={client}>
        <AddressBookSection />
      </QueryClientProvider>,
    );
    chooseFile("address-book.csv", FILE);

    await settle();
    expect(screen.getByLabelText("What the load changed")).toBeInTheDocument();
    expect(freshEntries(client, 7, shown)).toEqual([]);
  });

  it("lists every row a refused load names, each on its own line", async () => {
    post.mockRejectedValue(
      new ApiError(422, "row 3: service; row 7: identity", {
        type: "https://messagecrate.app/docs/developer/reference/errors/validation-failed",
        title: "Validation failed",
        status: 422,
        errors: [
          'row 3: service "imessage" is not one Message Crate stores; use phone or whatsapp',
          "row 7: identity is blank",
        ],
      }),
    );
    renderWithProviders(<AddressBookSection />);
    chooseFile("address-book.csv", FILE);
    await settle();

    const alert = screen.getByRole("alert");
    expect(
      within(alert)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual([
      'Row 3: service "imessage" is not one Message Crate stores; use phone or whatsapp',
      "Row 7: identity is blank",
    ]);
    expect(alert.textContent).toContain("Nothing was loaded.");
  });

  it("marks the contacts stale after a refused load too, as every write does", async () => {
    const shown = [keys.contacts.all];
    const client = testQueryClient();
    seedEntries(client, 7, shown);
    post.mockRejectedValue(new Error("address book is empty"));
    render(
      <QueryClientProvider client={client}>
        <AddressBookSection />
      </QueryClientProvider>,
    );
    chooseFile("address-book.csv", FILE);

    await settle();
    expect(screen.getByText("Address book is empty")).toBeInTheDocument();
    expect(freshEntries(client, 7, shown)).toEqual([]);
  });

  it("shows the reason when the load fails some other way", async () => {
    post.mockRejectedValue(new Error("address book is empty"));
    renderWithProviders(<AddressBookSection />);
    chooseFile("address-book.csv", "  ");
    await settle();

    expect(screen.getByText("Address book is empty")).toBeInTheDocument();
  });

  it("refuses a file past the size the server accepts, without asking the server", async () => {
    renderWithProviders(<AddressBookSection />);
    const input = screen.getByLabelText("Address book file") as HTMLInputElement;
    const file = new File(["x"], "huge.csv");
    Object.defineProperty(file, "size", { value: 9 * MIB });
    fireEvent.change(input, { target: { files: [file] } });

    await settle();
    expect(screen.getByText("That file is larger than 8 MB.")).toBeInTheDocument();
    expect(post).not.toHaveBeenCalled();
  });
});
