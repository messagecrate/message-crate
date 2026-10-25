/** @vitest-environment jsdom */

/**
 * Regression test: the Groups menu's checkmarks used to come from a stale
 * snapshot once the checked contact's own group membership emptied the
 * checked set (unticking the only group on the only checked contact, on
 * that group's own page, drops the row out of the list). The menu fell back
 * to the pre-write snapshot and kept showing the group as ticked forever.
 * The fix resolves that fallback against the live contact list instead of
 * the stale rows, so a membership write the menu itself caused is reflected
 * right away.
 */

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, type MockInstance, vi } from "vitest";
import ContactHistoryProvider from "../components/ContactHistoryProvider";
import RightPane from "../components/RightPane";
import { RightToolbarProvider } from "../components/RightToolbarContext";
import { ApiError } from "../lib/api";
import { groupListQuery } from "../lib/contactGroups";
import { PAGE_SIZE_FIRST } from "../lib/listPaging";
import { advanceTimersAndSettle, settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { Providers } from "../test/providers";
import ContactList, { FILTER_DEBOUNCE_MS } from "./ContactList";

vi.mock("../hooks/useAuth", async () => (await import("../test/providers")).authModule());

const tauriMock = vi.hoisted(() => ({ current: false }));
vi.mock("../lib/tauriCheck", () => ({ isTauri: () => tauriMock.current }));

vi.mock("../lib/serverApi", () => ({
  listContacts: vi.fn(),
  listContactGroups: vi.fn(),
  createContactGroup: vi.fn(),
  updateContactGroup: vi.fn(),
  deleteContactGroup: vi.fn(),
  updateContactGroupMembers: vi.fn(),
  exportAddressBook: vi.fn(),
  trashContact: vi.fn(),
  restoreContact: vi.fn(),
  getContact: vi.fn(),
  mergeContacts: vi.fn(),
  unmergeContacts: vi.fn(),
}));

vi.mock("../lib/saveFile", () => ({ saveFile: vi.fn().mockResolvedValue(true) }));

// A searched list's first page, such as a Contact Group's, holds 40 rows
// (PAGE_SIZE_FIRST in lib/listPaging.ts). The mock below sets it to 15, so a
// test of the rows past the first page draws 20 rows rather than 120, and
// stays well inside Vitest's test timeout on a busy machine (#2955). It must
// stay above the 10 rows from the end at which the list asks for the next
// page (NEAR_END_THRESHOLD in InfiniteOffsetList.tsx), because jsdom lays
// nothing out and every row reads as on screen, so a first page of 10 or
// fewer loads the rest by itself.
vi.mock("../lib/listPaging", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/listPaging")>()),
  PAGE_SIZE_FIRST: 15,
}));

import { saveFile } from "../lib/saveFile";
import {
  createContactGroup,
  exportAddressBook,
  getContact,
  listContactGroups,
  listContacts,
  mergeContacts,
  restoreContact,
  trashContact,
  unmergeContacts,
  updateContactGroupMembers,
} from "../lib/serverApi";
import { nth } from "../test/nth";
import { setupUser } from "../test/user";

const listContactsMock = vi.mocked(listContacts);
const listContactGroupsMock = vi.mocked(listContactGroups);
const updateMembersMock = vi.mocked(updateContactGroupMembers);
const exportMock = vi.mocked(exportAddressBook);
const createGroupMock = vi.mocked(createContactGroup);
const trashMock = vi.mocked(trashContact);
const restoreMock = vi.mocked(restoreContact);

/** True once the server has actually dropped Alice's Family membership. */
let familyRemoved = false;

beforeEach(() => {
  startFakeClock();
  vi.clearAllMocks();
  familyRemoved = false;
  // Mirrors what the server would answer: the write below flips this, and the
  // invalidate the mutation issues on settling refetches this same mock, so
  // the test also proves the optimistic patch and the server truth agree —
  // not just the moment right after the click.
  listContactsMock.mockImplementation(
    async () =>
      ({
        items: [
          {
            id: 1,
            name: "Alice",
            identity_count: 1,
            addresses: [],
            groups: familyRemoved ? [] : ["Family"],
          },
        ],
        total: 1,
        limit: 200,
        offset: 0,
      }) as unknown as Awaited<ReturnType<typeof listContacts>>,
  );
  listContactGroupsMock.mockResolvedValue([{ id: 10, name: "Family" }]);
});

afterEach(() => {
  cleanup();
  stopFakeClock();
});

/** Open the ⋯ menu and choose one of its items. */
async function chooseAction(name: string) {
  const user = setupUser();
  await user.click(screen.getByRole("button", { name: "Contact actions" }));
  await user.click(screen.getByRole("menuitem", { name }));
}

/** Renders the list inside the providers and panes it needs on the page. */
function renderList(props: Partial<ComponentProps<typeof ContactList>> = {}) {
  return render(
    <Providers>
      <ContactHistoryProvider>
        <RightToolbarProvider>
          <RightPane>
            <ContactList onSelect={() => {}} {...props} />
          </RightPane>
        </RightToolbarProvider>
      </ContactHistoryProvider>
    </Providers>,
  );
}

describe("ContactList", () => {
  it("keeps the Groups menu's checkmark live after unticking the group the page is filtered to", async () => {
    updateMembersMock.mockImplementation(async () => {
      familyRemoved = true;
      return { added: 0, removed: 1 };
    });

    renderList({ groupFilter: "Family" });
    await settle();

    const rowCheckbox = screen.getByRole("checkbox", { name: "Select Alice" });
    act(() => {
      rowCheckbox.click();
    });
    expect(rowCheckbox).toBeChecked();

    const menuButton = screen.getByRole("button", { name: "Contact Groups" });
    act(() => {
      menuButton.click();
    });

    const familyCheckbox = screen.getByRole("checkbox", { name: "Family" });
    expect(familyCheckbox).toBeChecked();

    // Untick Family on the only checked contact, on the Family group page
    // itself: the row leaves the list, the checked set empties, and the menu
    // falls back to its last-known targets.
    act(() => {
      familyCheckbox.click();
    });

    await settle();
    expect(updateMembersMock).toHaveBeenCalledWith(10, { add: [], remove: [1] });
    expect(screen.getByRole("checkbox", { name: "Family" })).not.toBeChecked();
  });

  it("shows the server's refusal of a Contact Group created from the Groups menu", async () => {
    const refusal =
      'name can\'t hold ";", because the address book separates Contact Group names with it';
    createGroupMock.mockRejectedValue(new ApiError(422, refusal));
    const user = setupUser();

    renderList();
    await settle();

    await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    await user.click(screen.getByRole("button", { name: /Create Contact Group$/ }));
    await user.type(screen.getByPlaceholderText("Contact Group name"), "Work; 2024{Enter}");
    await settle();

    expect(createGroupMock).toHaveBeenCalledWith({ name: "Work; 2024" });
    expect(
      screen.getByText(
        'Name can\'t hold ";", because the address book separates Contact Group names with it',
      ),
    ).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Contact Group name")).toHaveValue("Work; 2024");
    expect(updateMembersMock).not.toHaveBeenCalled();
  });

  it("refuses a reserved Contact Group name from the Groups menu without asking the server to create it", async () => {
    const user = setupUser();

    renderList();
    await settle();

    await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    await user.click(screen.getByRole("button", { name: /Create Contact Group$/ }));
    await user.type(screen.getByPlaceholderText("Contact Group name"), "Trash{Enter}");

    await settle();
    expect(screen.getByText("Trash is a reserved Contact Group")).toBeInTheDocument();
    expect(createGroupMock).not.toHaveBeenCalled();
    expect(updateMembersMock).not.toHaveBeenCalled();
  });

  it("exports the group the list shows, or the checked rows when there are any", async () => {
    exportMock.mockResolvedValue("contact_id,display_name,service,identity,groups\n");
    renderList({ groupFilter: "Family" });
    await settle();
    const rowCheckbox = screen.getByRole("checkbox", { name: "Select Alice" });

    await chooseAction("Export Contacts");
    await settle();
    expect(saveFile).toHaveBeenCalledTimes(1);
    // The same search the list sends the server for this group.
    expect(exportMock).toHaveBeenLastCalledWith({ q: groupListQuery("Family", "") });

    fireEvent.click(rowCheckbox);
    expect(rowCheckbox).toBeChecked();
    await chooseAction("Export Contacts");
    await settle();
    expect(saveFile).toHaveBeenCalledTimes(2);
    expect(exportMock).toHaveBeenLastCalledWith({ ids: [1] });
  });

  it("clears every Contact Group on the checked contacts at once", async () => {
    listContactsMock.mockResolvedValue({
      items: ["Alice", "Bob"].map((name, i) => ({
        id: i + 1,
        name,
        identity_count: 1,
        addresses: [],
        groups: ["Family", "Work"],
      })),
      total: 2,
      limit: 200,
      offset: 0,
    } as unknown as Awaited<ReturnType<typeof listContacts>>);
    listContactGroupsMock.mockResolvedValue([
      { id: 10, name: "Family" },
      { id: 11, name: "Work" },
    ]);
    // The first write never answers, so Clear all reaches the second Contact
    // Group only if it does not wait for the first.
    updateMembersMock.mockReturnValueOnce(new Promise(() => {}));
    updateMembersMock.mockResolvedValue({ added: 0, removed: 2 });

    renderList();
    await settle();
    const user = setupUser();
    await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
    await user.click(screen.getByRole("checkbox", { name: "Select Bob" }));
    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    await user.click(screen.getByRole("button", { name: "Clear all" }));

    await settle();
    expect(updateMembersMock).toHaveBeenCalledTimes(2);
    const writes = updateMembersMock.mock.calls
      .map(([id, body]) => ({ id, remove: [...(body?.remove ?? [])].sort((a, b) => a - b) }))
      .sort((a, b) => a.id - b.id);
    expect(writes).toEqual([
      { id: 10, remove: [1, 2] },
      { id: 11, remove: [1, 2] },
    ]);
  });

  it("checks every contact between a shift-click and the last clicked contact", async () => {
    const names = ["Alice", "Bob", "Carol", "Dave", "Erin"];
    listContactsMock.mockResolvedValue({
      items: names.map((name, i) => ({
        id: i + 1,
        name,
        identity_count: 1,
        addresses: [],
        groups: [],
      })),
      total: names.length,
      limit: 200,
      offset: 0,
    } as unknown as Awaited<ReturnType<typeof listContacts>>);

    renderList();

    const box = (name: string) => screen.getByRole("checkbox", { name: `Select ${name}` });
    const checkedNames = () => names.filter((name) => (box(name) as HTMLInputElement).checked);

    await settle();
    fireEvent.click(box("Bob"));
    expect(checkedNames()).toEqual(["Bob"]);

    // Checks from the last clicked row to the shift-clicked one.
    fireEvent.click(box("Erin"), { shiftKey: true });
    expect(checkedNames()).toEqual(["Bob", "Carol", "Dave", "Erin"]);

    // Unchecks the same way: uncheck one end, shift-click the other.
    fireEvent.click(box("Dave"));
    fireEvent.click(box("Carol"), { shiftKey: true });
    expect(checkedNames()).toEqual(["Bob", "Erin"]);

    // The range starts at the last clicked row (Carol), not at the furthest
    // checked one (Erin), so Dave stays unchecked.
    fireEvent.click(box("Alice"), { shiftKey: true });
    expect(checkedNames()).toEqual(["Alice", "Bob", "Carol", "Erin"]);
  });

  it("lists the server's Unknown and No group contacts once the full list is loaded", async () => {
    // Bob and Dave are Unknown; Dave also has a stored group. Alice is the
    // only contact that is neither Unknown nor in a stored group.
    const rows = [
      { id: 1, name: "Alice", groups: [], unknown: false },
      { id: 2, name: "Bob", groups: [], unknown: true },
      { id: 3, name: "Carol", groups: ["Family"], unknown: false },
      { id: 4, name: "Dave", groups: ["Family"], unknown: true },
    ];
    // Answers the way the server does, so the list is right whether the
    // browser filters the loaded rows or asks again.
    listContactsMock.mockImplementation(async ({ q } = {}) => {
      const items = rows
        .filter((row) => {
          if (q === "group:unknown") return row.unknown;
          if (q === "group:none") return row.groups.length === 0 && !row.unknown;
          return true;
        })
        .map((row) => ({ ...row, identity_count: 1, addresses: [] }));
      return { items, total: items.length, limit: 200, offset: 0 } as unknown as Awaited<
        ReturnType<typeof listContacts>
      >;
    });

    const page = (groupFilter: string | null) => (
      <Providers>
        <ContactHistoryProvider>
          <RightToolbarProvider>
            <RightPane>
              <ContactList groupFilter={groupFilter} onSelect={() => {}} />
            </RightPane>
          </RightToolbarProvider>
        </ContactHistoryProvider>
      </Providers>
    );
    const listed = () =>
      rows
        .map((row) => row.name)
        .filter((name) => screen.queryByRole("checkbox", { name: `Select ${name}` }) !== null);

    const { rerender } = render(page(null));
    await settle();
    expect(listed()).toEqual(["Alice", "Bob", "Carol", "Dave"]);

    rerender(page("unknown"));
    await settle();
    expect(listed()).toEqual(["Bob", "Dave"]);

    rerender(page("none"));
    await settle();
    expect(listed()).toEqual(["Alice"]);
  });

  describe("the search the server is asked", () => {
    const page = (filter: string, groupFilter: string | null = null) => (
      <Providers>
        <ContactHistoryProvider>
          <RightToolbarProvider>
            <RightPane>
              <ContactList filter={filter} groupFilter={groupFilter} onSelect={() => {}} />
            </RightPane>
          </RightToolbarProvider>
        </ContactHistoryProvider>
      </Providers>
    );

    const asked = () => listContactsMock.mock.calls.map(([params]) => params?.q);

    it("asks once, for the search as it stands when typing stops", async () => {
      const { rerender } = render(page(""));
      await settle();
      expect(asked()).toEqual([""]);

      // A `word:` token only the server can apply, so the loaded list is not enough.
      rerender(page("name:a"));
      await advanceTimersAndSettle(FILTER_DEBOUNCE_MS - 1);
      rerender(page("name:al"));
      await advanceTimersAndSettle(FILTER_DEBOUNCE_MS - 1);
      rerender(page("name:ali"));
      await advanceTimersAndSettle(FILTER_DEBOUNCE_MS - 1);
      expect(asked()).toEqual([""]);

      await advanceTimersAndSettle(1);
      expect(asked()).toEqual(["", "name:ali"]);
    });

    it("loads the full list first when it opens on a search", async () => {
      render(page("ali"));
      await advanceTimersAndSettle(FILTER_DEBOUNCE_MS);
      expect(asked()).toEqual([""]);
    });

    it("keeps the full list a cleared search went back to while the next search waits", async () => {
      const { rerender } = render(page(""));
      await settle();
      rerender(page("name:a"));
      await advanceTimersAndSettle(FILTER_DEBOUNCE_MS);
      expect(asked()).toEqual(["", "name:a"]);

      // Cleared, then a new search typed before the old one's wait would have ended.
      rerender(page(""));
      await advanceTimersAndSettle(FILTER_DEBOUNCE_MS / 3);
      rerender(page("name:b"));
      await advanceTimersAndSettle(FILTER_DEBOUNCE_MS - 1);
      expect(asked()).toEqual(["", "name:a", ""]);

      await advanceTimersAndSettle(1);
      expect(asked()).toEqual(["", "name:a", "", "name:b"]);
    });

    it("keeps the full list when the next search is typed before any timer runs", async () => {
      const { rerender } = render(page(""));
      await settle();
      rerender(page("name:a"));
      await advanceTimersAndSettle(FILTER_DEBOUNCE_MS);

      // Cleared, then a new search typed with no timer run in between.
      rerender(page(""));
      rerender(page("name:b"));
      await advanceTimersAndSettle(FILTER_DEBOUNCE_MS - 1);
      expect(asked()).toEqual(["", "name:a", ""]);

      await advanceTimersAndSettle(1);
      expect(asked()).toEqual(["", "name:a", "", "name:b"]);
    });

    it("asks for a Contact Group's page without waiting", async () => {
      render(page("", "Family"));
      await settle();
      expect(asked().at(-1)).toBe(groupListQuery("Family", ""));
    });

    it("stops asking once the search is cleared before the wait ends", async () => {
      const { rerender } = render(page(""));
      await settle();

      rerender(page("name:al"));
      await advanceTimersAndSettle(FILTER_DEBOUNCE_MS - 1);
      rerender(page(""));
      await advanceTimersAndSettle(FILTER_DEBOUNCE_MS);
      expect(asked()).toEqual([""]);
    });
  });

  it("names the search box's text a search when no contact matches it", async () => {
    renderList({ filter: "zzz" });

    await settle();
    expect(screen.getByText("No contacts match this search")).toBeInTheDocument();
  });

  describe("in the desktop list", () => {
    let heights: MockInstance;
    let widths: MockInstance;
    beforeEach(() => {
      tauriMock.current = true;
      // jsdom lays out nothing; give React Aria's Virtualizer a viewport to fill.
      heights = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
      widths = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(300);
    });

    afterEach(() => {
      heights.mockRestore();
      widths.mockRestore();
      tauriMock.current = false;
    });

    it("checks a contact from its avatar without opening it", async () => {
      const onSelect = vi.fn();
      renderList({ onSelect });
      await settle();
      const box = screen.getByRole("checkbox", { name: "Select Alice" });
      const avatar = box.closest("label");
      expect(avatar).not.toBeNull();

      const user = setupUser();
      await user.click(avatar as HTMLElement);

      expect(box).toBeChecked();
      expect(onSelect).not.toHaveBeenCalled();
    });

    it("checks a contact from its checkbox input, which lies over the drawn box (#3251)", async () => {
      const onSelect = vi.fn();
      renderList({ onSelect });
      await settle();
      const box = screen.getByRole("checkbox", { name: "Select Alice" });
      // The input shares the drawn box's frame rather than sitting in a
      // clipped 1px span, so a press on the box lands on the input. jsdom
      // does no hit testing, so these two lines are what guard that layout:
      // the clicks below pass either way.
      expect(box.nextElementSibling).toHaveClass("mc-list-check");
      expect(box.parentElement).toHaveClass("mc-list-check-frame");

      const user = setupUser();
      await user.click(box);
      expect(box).toBeChecked();
      await user.click(box);
      expect(box).not.toBeChecked();
      expect(onSelect).not.toHaveBeenCalled();
    });
  });

  it("marks only the open contact as current while other contacts are checked", async () => {
    listContactsMock.mockResolvedValue({
      items: ["Alice", "Bob", "Carol"].map((name, i) => ({
        id: i + 1,
        name,
        identity_count: 1,
        addresses: [],
        groups: [],
      })),
      total: 3,
      limit: 200,
      offset: 0,
    } as unknown as Awaited<ReturnType<typeof listContacts>>);

    renderList({ selectedId: "1" });

    const current = () =>
      screen
        .getAllByRole("button")
        .filter((el) => el.hasAttribute("aria-current"))
        .map((el) => el.textContent);

    await settle();
    const bob = screen.getByRole("checkbox", { name: "Select Bob" });
    expect(current()).toEqual(["Alice"]);

    // Checking Bob draws the checked rows highlighted, not the open one,
    // but the open contact is still Alice.
    fireEvent.click(bob);
    expect(bob).toBeChecked();
    expect(current()).toEqual(["Alice"]);
  });

  describe("Select all over more contacts than the first page", () => {
    // The first page and five contacts past it.
    const everyone = Array.from({ length: PAGE_SIZE_FIRST + 5 }, (_, i) => ({
      id: i + 1,
      name: `Person ${String(i + 1).padStart(4, "0")}`,
      identity_count: 1,
      addresses: [],
      groups: ["Family"],
    }));

    beforeEach(() => {
      // The server's paging: a group page's first page holds PAGE_SIZE_FIRST of
      // the PAGE_SIZE_FIRST + 5 in `everyone`.
      listContactsMock.mockImplementation(
        async ({ limit = PAGE_SIZE_FIRST, offset = 0 }) =>
          ({
            items: everyone.slice(offset, offset + limit),
            total: everyone.length,
            limit,
            offset,
          }) as unknown as Awaited<ReturnType<typeof listContacts>>,
      );
      exportMock.mockResolvedValue("contact_id,display_name\n");
    });

    function renderAll() {
      renderList({ groupFilter: "Family" });
    }

    it("does not read as all while contacts past the loaded ones are unticked", async () => {
      renderAll();
      await settle();
      // Every row of the first page, ticked by hand with one Shift + click.
      const rows = screen.getAllByRole("checkbox", { name: /^Select Person/ });
      fireEvent.click(nth(rows, 0));
      fireEvent.click(nth(rows, rows.length - 1), { shiftKey: true });
      expect(rows[rows.length - 1]).toBeChecked();
      expect(screen.getByRole("checkbox", { name: "Select all contacts" })).not.toBeChecked();
    });

    it("exports every contact the list holds", async () => {
      renderAll();
      await settle();

      const box = screen.getByRole("checkbox", { name: "Select all contacts" });
      // Select all reads the contacts past the first page from a mock that
      // answers at once, then draws every tick in a transition. `settle` lands
      // both, so the box is ticked by then.
      fireEvent.click(box);
      await settle();
      expect(box).toBeChecked();

      await chooseAction("Export Contacts");
      await settle();
      expect(exportMock).toHaveBeenCalled();
      const body = exportMock.mock.calls.at(-1)?.[0] as { ids?: number[] } | undefined;
      const ids = body?.ids ?? [];
      expect([...ids].sort((a, b) => a - b)).toEqual(everyone.map((c) => c.id));
    });

    it("waits while the pages load, and says why when they do not", async () => {
      let refuse!: (error: Error) => void;
      listContactsMock.mockImplementation(async ({ limit = PAGE_SIZE_FIRST, offset = 0 }) => {
        if (offset > 0) {
          return new Promise((_, reject) => {
            refuse = reject;
          });
        }
        return {
          items: everyone.slice(offset, offset + limit),
          total: everyone.length,
          limit,
          offset,
        } as unknown as Awaited<ReturnType<typeof listContacts>>;
      });
      renderAll();
      await settle();

      const box = screen.getByRole("checkbox", { name: "Select all contacts" });
      fireEvent.click(box);
      await settle();
      expect(box).toBeDisabled();

      refuse(new Error("offset is past the end of the list"));
      await settle();
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Select all could not read every row: offset is past the end of the list",
      );
      expect(box).not.toBeDisabled();
      expect(box).not.toBeChecked();
      expect(screen.getByRole("checkbox", { name: "Select Person 0001" })).not.toBeChecked();
    });
  });

  describe("the ⋯ menu and Undo", () => {
    beforeEach(() => {
      trashMock.mockResolvedValue(undefined);
      restoreMock.mockResolvedValue(undefined);
      updateMembersMock.mockResolvedValue({ added: 1, removed: 1 });
    });

    async function openMenu() {
      await setupUser().click(screen.getByRole("button", { name: "Contact actions" }));
    }

    it("names what Export and Delete act on, and offers Delete only for checked rows", async () => {
      const user = setupUser();
      renderList();
      await settle();
      await openMenu();
      expect(screen.getByText("All contacts")).toBeInTheDocument();
      expect(screen.getByRole("menuitem", { name: "Delete Contacts" })).toHaveAttribute(
        "aria-disabled",
        "true",
      );
      expect(screen.getByRole("menuitem", { name: "Undo" })).toHaveAttribute(
        "aria-disabled",
        "true",
      );
      expect(screen.getByRole("menuitem", { name: "Redo" })).toHaveAttribute(
        "aria-disabled",
        "true",
      );
      await user.keyboard("{Escape}");

      await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
      await openMenu();
      expect(screen.getByText("1 selected")).toBeInTheDocument();
      expect(screen.getByRole("menuitem", { name: "Delete Contacts" })).not.toHaveAttribute(
        "aria-disabled",
      );
    });

    it("names a searched or grouped list This list", async () => {
      renderList({ groupFilter: "Family" });
      await settle();
      await openMenu();
      expect(screen.getByText("This list")).toBeInTheDocument();
    });

    it("moves the checked contacts to Trash after asking, and Ctrl+Z restores them", async () => {
      const user = setupUser();
      renderList();
      await settle();
      await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
      await chooseAction("Delete Contacts");

      expect(screen.getByRole("dialog", { name: "Move 1 contact to Trash?" })).toBeInTheDocument();
      expect(trashMock).not.toHaveBeenCalled();
      await user.click(screen.getByRole("button", { name: "Move to Trash" }));
      await settle();

      expect(trashMock).toHaveBeenCalledWith(1);
      expect(screen.getByRole("status")).toHaveTextContent("Moved Alice to Trash");
      expect(screen.getByRole("checkbox", { name: "Select Alice" })).not.toBeChecked();
      await openMenu();
      expect(screen.getByRole("menuitem", { name: "Undo Delete Contacts" })).toBeInTheDocument();
      await user.keyboard("{Escape}");

      await user.keyboard("{Control>}z{/Control}");
      await settle();
      expect(restoreMock).toHaveBeenCalledWith(1);
      expect(screen.getByRole("status")).toHaveTextContent("Restored Alice");

      await user.keyboard("{Control>}y{/Control}");
      await settle();
      expect(trashMock).toHaveBeenCalledTimes(2);
      expect(screen.getByRole("status")).toHaveTextContent("Moved Alice to Trash");
    });

    it("leaves Ctrl+Z to a text field that has focus", async () => {
      const user = setupUser();
      render(<input aria-label="Search" />);
      renderList();
      await settle();
      await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
      await chooseAction("Delete Contacts");
      await user.click(screen.getByRole("button", { name: "Move to Trash" }));
      await settle();

      await user.click(screen.getByRole("textbox", { name: "Search" }));
      await user.keyboard("{Control>}z{/Control}");
      await settle();
      expect(restoreMock).not.toHaveBeenCalled();
    });

    it("undoes with Ctrl+Z while focus is in the docked contact pane", async () => {
      const user = setupUser();
      render(
        <aside role="dialog" aria-label="Alice">
          <button type="button">Edit name</button>
        </aside>,
      );
      renderList();
      await settle();
      await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
      await chooseAction("Delete Contacts");
      await user.click(screen.getByRole("button", { name: "Move to Trash" }));
      await settle();

      await user.click(screen.getByRole("button", { name: "Edit name" }));
      await user.keyboard("{Control>}z{/Control}");
      await settle();
      expect(restoreMock).toHaveBeenCalledWith(1);
    });

    it("leaves Ctrl+Z alone while the Delete Contacts dialog is open", async () => {
      const user = setupUser();
      renderList();
      await settle();
      await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
      await chooseAction("Delete Contacts");
      await user.click(screen.getByRole("button", { name: "Move to Trash" }));
      await settle();

      await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
      await chooseAction("Delete Contacts");
      expect(screen.getByRole("dialog")).toBeInTheDocument();
      await user.keyboard("{Control>}z{/Control}");
      await settle();
      expect(restoreMock).not.toHaveBeenCalled();
    });

    it("records a Contact Group change made from the list, and Undo sends the opposite", async () => {
      const user = setupUser();
      renderList();
      await settle();
      await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
      await user.click(screen.getByRole("button", { name: "Contact Groups" }));
      await user.click(screen.getByRole("checkbox", { name: "Family" }));
      await settle();
      expect(updateMembersMock).toHaveBeenLastCalledWith(10, { add: [], remove: [1] });
      expect(screen.getByRole("status")).toHaveTextContent("Removed Alice from Family");
      await user.keyboard("{Escape}");

      await chooseAction("Undo Remove from Contact Group");
      await settle();
      expect(updateMembersMock).toHaveBeenLastCalledWith(10, { add: [1], remove: [] });
      expect(screen.getByRole("status")).toHaveTextContent("Added Alice to Family");
    });

    it("says why an undo's Contact Group was refused, and shows the server's groups after it", async () => {
      // What the server holds for Alice. The undo's add is refused, and by
      // then someone else has put her in Work, which neither the optimistic
      // chips nor the snapshot taken before the request know about.
      let aliceGroups = ["Family"];
      listContactsMock.mockImplementation(
        async () =>
          ({
            items: [
              { id: 1, name: "Alice", identity_count: 1, addresses: [], groups: aliceGroups },
            ],
            total: 1,
            limit: 200,
            offset: 0,
          }) as unknown as Awaited<ReturnType<typeof listContacts>>,
      );
      listContactGroupsMock.mockResolvedValue([
        { id: 10, name: "Family" },
        { id: 11, name: "Work" },
      ]);
      updateMembersMock.mockImplementation(async (_id, patch) => {
        if ((patch.remove ?? []).length > 0) {
          aliceGroups = [];
          return { added: 0, removed: 1 };
        }
        aliceGroups = ["Work"];
        throw new ApiError(404, "Contact Group not found");
      });
      const user = setupUser();
      renderList();
      await settle();
      await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
      await user.click(screen.getByRole("button", { name: "Contact Groups" }));
      await user.click(screen.getByRole("checkbox", { name: "Family" }));
      await settle();
      await user.keyboard("{Escape}");

      await chooseAction("Undo Remove from Contact Group");
      await settle();
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Could not add Alice to Family: Contact Group not found",
      );
      await user.click(screen.getByRole("button", { name: "Contact Groups" }));
      expect(screen.getByRole("checkbox", { name: "Family" })).not.toBeChecked();
      expect(screen.getByRole("checkbox", { name: "Work" })).toBeChecked();
    });

    it("says why an export failed in the list's banner", async () => {
      exportMock.mockRejectedValue(new Error("the search has a word Contacts does not know"));
      renderList();
      await settle();
      await chooseAction("Export Contacts");
      await settle();
      expect(screen.getByRole("alert")).toHaveTextContent(
        "The search has a word Contacts does not know",
      );
      expect(saveFile).not.toHaveBeenCalled();
    });
  });

  describe("Merge Contacts", () => {
    const people = [
      { id: 1, name: "Nicholas Koch", addresses: ["+12155550112"] },
      { id: 2, name: "Nick Koch", addresses: ["nick.koch@example.com"] },
      { id: 3, name: "", addresses: ["+12155550113"] },
    ];

    beforeEach(() => {
      listContactsMock.mockImplementation(
        async () =>
          ({
            items: people.map((p) => ({ ...p, identity_count: 1, groups: [] })),
            total: people.length,
            limit: 200,
            offset: 0,
          }) as unknown as Awaited<ReturnType<typeof listContacts>>,
      );
      vi.mocked(getContact).mockImplementation(
        async (id) =>
          ({
            id: Number(id),
            name: people.find((p) => p.id === Number(id))?.name ?? "",
            identities: [],
            total_messages: 1,
          }) as unknown as Awaited<ReturnType<typeof getContact>>,
      );
      vi.mocked(mergeContacts).mockResolvedValue({} as Awaited<ReturnType<typeof mergeContacts>>);
      vi.mocked(unmergeContacts).mockResolvedValue(
        {} as Awaited<ReturnType<typeof unmergeContacts>>,
      );
    });

    it("is offered for two or more checked contacts", async () => {
      const user = setupUser();
      renderList();
      await settle();
      await user.click(screen.getByRole("checkbox", { name: "Select Nicholas Koch" }));
      await user.click(screen.getByRole("button", { name: "Contact actions" }));
      expect(screen.getByRole("menuitem", { name: "Merge Contacts" })).toHaveAttribute(
        "aria-disabled",
        "true",
      );
      await user.keyboard("{Escape}");
      await user.click(screen.getByRole("checkbox", { name: "Select Nick Koch" }));
      await user.click(screen.getByRole("button", { name: "Contact actions" }));
      expect(screen.getByRole("menuitem", { name: "Merge Contacts" })).not.toHaveAttribute(
        "aria-disabled",
      );
    });

    it("merges into the contact whose name is kept, opens it, and Undo unmerges", async () => {
      const user = setupUser();
      const onSelect = vi.fn();
      renderList({ onSelect });
      await settle();
      for (const name of ["Nicholas Koch", "Nick Koch", "+12155550113"]) {
        await user.click(screen.getByRole("checkbox", { name: `Select ${name}` }));
      }
      await chooseAction("Merge Contacts");
      await settle();
      expect(screen.getByRole("dialog", { name: "Merge 3 contacts" })).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: /Name to keep/ }));
      await user.click(screen.getByRole("option", { name: /^Nicholas Koch/ }));
      await user.click(screen.getByRole("button", { name: "Merge" }));
      await settle();

      // In the order the list shows them, the nameless one first.
      expect(mergeContacts).toHaveBeenCalledWith(1, [3, 2]);
      expect(screen.queryByRole("dialog", { name: "Merge 3 contacts" })).toBeNull();
      expect(screen.getByRole("status")).toHaveTextContent("Merged 3 contacts into Nicholas Koch");
      expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: "1" }));
      expect(screen.getByRole("checkbox", { name: "Select Nick Koch" })).not.toBeChecked();

      await chooseAction("Undo Merge Contacts");
      await settle();
      expect(unmergeContacts).toHaveBeenCalledWith(1);
      expect(screen.getByRole("status")).toHaveTextContent(
        "Unmerged Nick Koch and 1 contact with no name",
      );
    });
  });
});
