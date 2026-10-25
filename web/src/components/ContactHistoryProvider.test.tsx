/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useClearContactHistory, useContactHistory } from "../hooks/useContactHistory";
import { ApiError, noAnswerError } from "../lib/api";
import {
  listContactGroups,
  updateContactGroupMembers,
  updateTrashedContacts,
} from "../lib/serverApi";
import { act } from "../test/act";
import { deferred } from "../test/deferred";
import { advanceTimersAndSettle, settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { Providers } from "../test/providers";
import { setupUser } from "../test/user";
import ContactHistoryProvider from "./ContactHistoryProvider";
import { UNDO_TOAST_MS } from "./ContactHistoryToast";

const auth = vi.hoisted(() => ({ accountId: 7 }));
vi.mock("../hooks/useAuth", async () =>
  (await import("../test/providers")).authModule(() => ({ accountId: auth.accountId })),
);

vi.mock("../lib/serverApi", () => ({
  TRASHED_CONTACT_IDS_MAX: 10_000,
  updateTrashedContacts: vi.fn(),
  listContactGroups: vi.fn(),
  createContactGroup: vi.fn(),
  updateContactGroup: vi.fn(),
  deleteContactGroup: vi.fn(),
  updateContactGroupMembers: vi.fn(),
}));

const trashMoveMock = vi.mocked(updateTrashedContacts);
type TrashMoveAnswer = Awaited<ReturnType<typeof updateTrashedContacts>>;
/** The bodies the Trash moves were sent with, in order. */
const trashMoves = () => trashMoveMock.mock.calls.map(([body]) => body);
const listGroupsMock = vi.mocked(listContactGroups);
const updateMembersMock = vi.mocked(updateContactGroupMembers);

const mother = { id: 1, label: "Mother" };
const ada = { id: 2, label: "Ada" };
const grace = { id: 3, label: "Grace" };

/** Buttons for what the Contacts list and the Trash do with the stack. */
function Harness() {
  const history = useContactHistory();
  const clear = useClearContactHistory();
  return (
    <div>
      <button type="button" onClick={() => void history.deleteContacts([mother, ada, grace])}>
        Delete three
      </button>
      <button type="button" onClick={() => void history.deleteContacts([mother])}>
        Delete Mother
      </button>
      <button type="button" onClick={() => history.push({ kind: "create", contact: mother })}>
        Add Mother
      </button>
      <button type="button" onClick={() => void history.redo()}>
        Redo
      </button>
      <button
        type="button"
        onClick={async () => {
          // One render's undo, called twice, as a held Ctrl+Z does before the
          // screen draws the first one's result.
          const { undo } = history;
          await undo();
          await undo();
        }}
      >
        Undo twice
      </button>
      <button
        type="button"
        onClick={() =>
          history.push({
            kind: "group",
            op: "add",
            changes: [{ group: "Family", contacts: [ada] }],
          })
        }
      >
        Add Ada to Family
      </button>
      <button
        type="button"
        onClick={() =>
          history.push({
            kind: "group",
            op: "remove",
            changes: [
              { group: "Family", contacts: [ada] },
              { group: "Work", contacts: [ada] },
              { group: "Club", contacts: [ada] },
            ],
          })
        }
      >
        Take Ada out of three
      </button>
      <button type="button" onClick={clear}>
        Delete for good
      </button>
      <p data-testid="undo">{history.undoEntry ? "can undo" : "nothing to undo"}</p>
      <p data-testid="redo">{history.redoEntry ? "can redo" : "nothing to redo"}</p>
      <p data-testid="report">{history.report ?? ""}</p>
    </div>
  );
}

function renderHarness() {
  return render(
    <Providers>
      <ContactHistoryProvider>
        <Harness />
      </ContactHistoryProvider>
    </Providers>,
  );
}

const toast = () => screen.queryByRole("status");

beforeEach(() => {
  startFakeClock();
  vi.clearAllMocks();
  auth.accountId = 7;
  // The server moves every contact it is sent, as it does when each is still where it was.
  trashMoveMock.mockImplementation(async (body) => ({
    trashed: body.add ?? [],
    restored: body.remove ?? [],
  }));
});

afterEach(() => {
  cleanup();
  stopFakeClock();
});

describe("ContactHistoryProvider", () => {
  it("trashes the contacts in one request, then shows a toast with Undo that goes after 20 seconds", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete three" }));
    await settle();

    expect(trashMoves()).toEqual([{ add: [1, 2, 3] }]);
    expect(toast()).toHaveTextContent("Moved 3 contacts to Trash");
    expect(screen.getByRole("button", { name: "Undo" })).toBeEnabled();

    await advanceTimersAndSettle(UNDO_TOAST_MS - 1);
    expect(toast()).not.toBeNull();
    await advanceTimersAndSettle(1);
    expect(toast()).toBeNull();
  });

  it("names one contact, and replaces the toast with the next action's", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete three" }));
    await settle();
    await advanceTimersAndSettle(UNDO_TOAST_MS - 1000);

    await user.click(screen.getByRole("button", { name: "Delete Mother" }));
    await settle();
    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(toast()).toHaveTextContent("Moved Mother to Trash");
    // The new toast has its own 20 seconds.
    await advanceTimersAndSettle(1000);
    expect(toast()).toHaveTextContent("Moved Mother to Trash");
  });

  it("closes the toast on Dismiss but not on Escape", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete Mother" }));
    await settle();

    await user.keyboard("{Escape}");
    expect(toast()).not.toBeNull();
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(toast()).toBeNull();
  });

  it("restores on Undo and says so without an Undo button; Redo trashes again", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete three" }));
    await settle();

    await user.click(screen.getByRole("button", { name: "Undo" }));
    await settle();
    expect(trashMoves()).toEqual([{ add: [1, 2, 3] }, { remove: [1, 2, 3] }]);
    expect(toast()).toHaveTextContent("Restored 3 contacts");
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
    expect(screen.getByTestId("redo")).toHaveTextContent("can redo");

    await user.click(screen.getByRole("button", { name: "Redo" }));
    await settle();
    expect(trashMoves().at(-1)).toEqual({ add: [1, 2, 3] });
    expect(toast()).toHaveTextContent("Moved 3 contacts to Trash");
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
    expect(screen.getByTestId("undo")).toHaveTextContent("can undo");
    expect(screen.getByTestId("redo")).toHaveTextContent("nothing to redo");
  });

  it("says which contacts a restore no longer found in the Trash, and drops the entry", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete three" }));
    await settle();

    // Ada left the Trash meanwhile, so the server restores the other two.
    trashMoveMock.mockResolvedValueOnce({ trashed: [], restored: [1, 3] });
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await settle();

    expect(trashMoves().at(-1)).toEqual({ remove: [1, 2, 3] });
    expect(screen.getByTestId("report")).toHaveTextContent(
      "Restored 2 contacts. 1 is no longer in the Trash.",
    );
    expect(screen.getByTestId("undo")).toHaveTextContent("nothing to undo");
    expect(screen.getByTestId("redo")).toHaveTextContent("nothing to redo");
    expect(toast()).toBeNull();
  });

  it("undoes New Contact by moving the contact to the Trash, and redoes it by restoring it", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Add Mother" }));
    expect(toast()).toHaveTextContent("Added Mother");

    await user.click(screen.getByRole("button", { name: "Undo" }));
    await settle();
    expect(trashMoves()).toEqual([{ add: [1] }]);
    expect(toast()).toHaveTextContent("Moved Mother to Trash");

    await user.click(screen.getByRole("button", { name: "Redo" }));
    await settle();
    expect(trashMoves()).toEqual([{ add: [1] }, { remove: [1] }]);
    expect(toast()).toHaveTextContent("Added Mother");
    expect(screen.getByTestId("undo")).toHaveTextContent("can undo");
  });

  it("keeps one contact deleted since the action out of a redo, and moves the rest", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete three" }));
    await settle();
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await settle();

    // Ada was deleted meanwhile, so the server refuses a move that names her.
    trashMoveMock.mockImplementation(async (body) => {
      if (body.add?.includes(2)) {
        throw new ApiError(422, "contact 2 not found", {
          type: "https://messagecrate.app/docs/developer/reference/errors/validation-failed",
          title: "Validation failed",
          status: 422,
          errors: ["contact 2 not found"],
        } as unknown as ConstructorParameters<typeof ApiError>[2]);
      }
      return { trashed: body.add ?? [], restored: body.remove ?? [] };
    });
    await user.click(screen.getByRole("button", { name: "Redo" }));
    await settle();

    expect(trashMoves().slice(2)).toEqual([{ add: [1, 2, 3] }, { add: [1, 3] }]);
    expect(screen.getByTestId("report")).toHaveTextContent(
      "Moved 2 contacts to Trash. 1 could not be moved.",
    );
  });

  it("says so when New Contact's undo could not move the contact to the Trash", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Add Mother" }));
    trashMoveMock.mockRejectedValueOnce(new ApiError(422, "contact 1 not found"));
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await settle();

    expect(screen.getByTestId("report")).toHaveTextContent("Could not move 1 contact to Trash.");
    expect(screen.getByTestId("undo")).toHaveTextContent("nothing to undo");
    expect(screen.getByTestId("redo")).toHaveTextContent("nothing to redo");
  });

  it("names each Contact Group that refused an undo, with the server's reason", async () => {
    listGroupsMock.mockResolvedValue([
      { id: 10, name: "Family" },
      { id: 11, name: "Work" },
      { id: 12, name: "Club" },
    ] as unknown as Awaited<ReturnType<typeof listContactGroups>>);
    updateMembersMock.mockImplementation(async (id) => {
      if (id === 10) throw new ApiError(404, "Contact Group not found");
      // A lost connection, as the client reports a request no server answered.
      if (id === 12) throw noAnswerError();
      return { added: 1, removed: 0 };
    });
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Take Ada out of three" }));
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await settle();

    expect(updateMembersMock.mock.calls.map(([id]) => id)).toEqual([10, 11, 12]);
    expect(noAnswerError().message).toMatch(/^The server at .* didn't answer\.$/);
    expect(screen.getByTestId("report").textContent?.split("\n")).toEqual([
      "Added Ada to 1 of 3 Contact Groups.",
      "Could not add Ada to Family: Contact Group not found",
      `Could not add Ada to Club: ${noAnswerError().message}`,
    ]);
    expect(screen.getByTestId("undo")).toHaveTextContent("nothing to undo");
    expect(screen.getByTestId("redo")).toHaveTextContent("nothing to redo");
    expect(toast()).toBeNull();
  });

  it("forgets both stacks on a delete for good", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete three" }));
    await settle();
    await user.click(screen.getByRole("button", { name: "Delete Mother" }));
    await settle();
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await settle();
    expect(screen.getByTestId("redo")).toHaveTextContent("can redo");

    await user.click(screen.getByRole("button", { name: "Delete for good" }));
    expect(screen.getByTestId("undo")).toHaveTextContent("nothing to undo");
    expect(screen.getByTestId("redo")).toHaveTextContent("nothing to redo");
    expect(toast()).toBeNull();
  });

  it("forgets the stack when the logged-in account changes", async () => {
    const user = setupUser();
    const { rerender } = renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete Mother" }));
    await settle();
    expect(screen.getByTestId("undo")).toHaveTextContent("can undo");

    auth.accountId = 8;
    rerender(
      <Providers>
        <ContactHistoryProvider>
          <Harness />
        </ContactHistoryProvider>
      </Providers>,
    );
    expect(screen.getByTestId("undo")).toHaveTextContent("nothing to undo");
    expect(toast()).toBeNull();
  });

  it("undoes the next entry down when one render's undo runs twice", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete three" }));
    await settle();
    await user.click(screen.getByRole("button", { name: "Delete Mother" }));
    await settle();

    await user.click(screen.getByRole("button", { name: "Undo twice" }));
    await settle();
    expect(trashMoves().slice(2)).toEqual([{ remove: [1] }, { remove: [1, 2, 3] }]);
    expect(screen.getByTestId("undo")).toHaveTextContent("nothing to undo");
  });

  it("has nothing to redo when a new action lands while an undo is in flight", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete Mother" }));
    await settle();

    const restored = deferred<TrashMoveAnswer>();
    trashMoveMock.mockReturnValueOnce(restored.promise);
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await user.click(screen.getByRole("button", { name: "Add Ada to Family" }));
    await act(async () => restored.resolve({ trashed: [], restored: [1] }));
    await settle();

    expect(screen.getByTestId("redo")).toHaveTextContent("nothing to redo");
  });

  it("puts a redo back on the undo stack, and keeps the newer toast, when a new action lands during it", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete Mother" }));
    await settle();
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await settle();

    const trashed = deferred<TrashMoveAnswer>();
    trashMoveMock.mockReturnValueOnce(trashed.promise);
    await user.click(screen.getByRole("button", { name: "Redo" }));
    await user.click(screen.getByRole("button", { name: "Add Ada to Family" }));
    await act(async () => trashed.resolve({ trashed: [1], restored: [] }));
    await settle();

    expect(screen.getByTestId("undo")).toHaveTextContent("can undo");
    expect(toast()).toHaveTextContent("Added Ada to Family");
    expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument();
  });
});
