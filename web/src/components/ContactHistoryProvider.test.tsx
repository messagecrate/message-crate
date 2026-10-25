/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useClearContactHistory, useContactHistory } from "../hooks/useContactHistory";
import { ApiError } from "../lib/api";
import { restoreContact, trashContact } from "../lib/serverApi";
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
  trashContact: vi.fn(),
  restoreContact: vi.fn(),
  listContactGroups: vi.fn(),
  createContactGroup: vi.fn(),
  updateContactGroup: vi.fn(),
  deleteContactGroup: vi.fn(),
  updateContactGroupMembers: vi.fn(),
}));

const trashMock = vi.mocked(trashContact);
const restoreMock = vi.mocked(restoreContact);

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
  trashMock.mockResolvedValue(undefined);
  restoreMock.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  stopFakeClock();
});

describe("ContactHistoryProvider", () => {
  it("trashes each contact, then shows a toast with Undo that goes after 20 seconds", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete three" }));
    await settle();

    expect(trashMock.mock.calls.map(([id]) => id)).toEqual([1, 2, 3]);
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
    expect(restoreMock.mock.calls.map(([id]) => id)).toEqual([1, 2, 3]);
    expect(toast()).toHaveTextContent("Restored 3 contacts");
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
    expect(screen.getByTestId("redo")).toHaveTextContent("can redo");

    await user.click(screen.getByRole("button", { name: "Redo" }));
    await settle();
    expect(trashMock).toHaveBeenCalledTimes(6);
    expect(toast()).toHaveTextContent("Moved 3 contacts to Trash");
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
    expect(screen.getByTestId("undo")).toHaveTextContent("can undo");
    expect(screen.getByTestId("redo")).toHaveTextContent("nothing to redo");
  });

  it("keeps going past a refused restore, says what happened, and drops the entry", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete three" }));
    await settle();

    restoreMock.mockImplementation(async (id) => {
      if (id === 2) throw new ApiError(404, "not found");
    });
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await settle();

    expect(restoreMock).toHaveBeenCalledTimes(3);
    expect(screen.getByTestId("report")).toHaveTextContent(
      "Restored 2 contacts. 1 is no longer in the Trash.",
    );
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
    expect(restoreMock.mock.calls.map(([id]) => id)).toEqual([1, 1, 2, 3]);
    expect(screen.getByTestId("undo")).toHaveTextContent("nothing to undo");
  });

  it("has nothing to redo when a new action lands while an undo is in flight", async () => {
    const user = setupUser();
    renderHarness();
    await user.click(screen.getByRole("button", { name: "Delete Mother" }));
    await settle();

    const restored = deferred<void>();
    restoreMock.mockReturnValue(restored.promise);
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await user.click(screen.getByRole("button", { name: "Add Ada to Family" }));
    await act(async () => restored.resolve());
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

    const trashed = deferred<void>();
    trashMock.mockReturnValue(trashed.promise);
    await user.click(screen.getByRole("button", { name: "Redo" }));
    await user.click(screen.getByRole("button", { name: "Add Ada to Family" }));
    await act(async () => trashed.resolve());
    await settle();

    expect(screen.getByTestId("undo")).toHaveTextContent("can undo");
    expect(toast()).toHaveTextContent("Added Ada to Family");
    expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument();
  });
});
