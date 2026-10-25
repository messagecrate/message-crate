/** @vitest-environment jsdom */

/**
 * Regression test for issue #295: on every render `ConversationList` handed
 * a freshly built `<TagsMenu>` to `RightToolbarContext`, whose provider
 * re-renders its whole subtree — `ConversationList` included — on every
 * write. The effect's dependencies (`targetConversations`, `tagChecks`, and
 * the `applyMembership` callback they feed) were rebuilt from a `conversations`
 * array that `useRoutePagedList` reallocated on every call, so the effect's
 * dependency array was never equal to the last render's, the effect fired
 * again after every one of those forced re-renders, and the two fed each
 * other into "Maximum update depth exceeded" before a person touched
 * anything.
 */

import { act, cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import RightPane from "../components/RightPane";
import { RightToolbarProvider } from "../components/RightToolbarContext";
import { ApiError } from "../lib/api";
import { createMessageTag, listConversations, updateMessageTagMembers } from "../lib/serverApi";
import type { Conversation } from "../lib/types";
import { settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { authModule, Providers } from "../test/providers";
import { setupUser } from "../test/user";
import ConversationList from "./ConversationList";

vi.mock("../hooks/useAuth", () => authModule());

vi.mock("../lib/serverApi", () => ({
  // messageTags.ts pulls slug helpers from contactGroups.ts, whose module-level
  // `createNameCollection` call needs these even though this test never uses them.
  listContactGroups: vi.fn().mockResolvedValue([]),
  createContactGroup: vi.fn(),
  updateContactGroup: vi.fn(),
  deleteContactGroup: vi.fn(),
  updateContactGroupMembers: vi.fn(),
  listMessageTags: vi.fn().mockResolvedValue([
    { id: 1, name: "Holiday" },
    { id: 2, name: "Receipts" },
  ]),
  createMessageTag: vi.fn(),
  updateMessageTag: vi.fn(),
  deleteMessageTag: vi.fn(),
  updateMessageTagMembers: vi.fn(),
  listConversations: vi.fn().mockResolvedValue({
    items: [
      { id: 1, display_name: "Alice", tags: ["Holiday"] },
      { id: 2, display_name: "Bob", tags: [] },
    ],
    total: 2,
    limit: 40,
    offset: 0,
  }),
}));

// jsdom has no ResizeObserver; VirtualList observes its scroll container on mount.
class StubResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

let consoleErrorSpy: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", StubResizeObserver);
  consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
  startFakeClock();
});

afterEach(() => {
  consoleErrorSpy.mockRestore();
  cleanup();
  // Also takes back the ResizeObserver stub.
  stopFakeClock();
});

/** Every logged message, joined, so one assertion covers every call shape. */
function loggedErrors(): string {
  return consoleErrorSpy.mock.calls
    .map((call: unknown[]) =>
      call.map((arg) => (typeof arg === "string" ? arg : String(arg))).join(" "),
    )
    .join("\n");
}

describe("ConversationList", () => {
  it("registers the tag menu into the right toolbar without looping", async () => {
    // React's nested-update guard reports a loop through console.error and
    // goes on looping, so `act` below would never return. Throwing from that
    // report ends the loop and fails the test.
    consoleErrorSpy.mockImplementation((...args: unknown[]) => {
      const text = String(args[0]);
      if (text.includes("Maximum update depth")) throw new Error(text);
    });
    // `act` runs every effect and every render they cause before it returns.
    await act(async () => {
      render(
        <Providers>
          <MemoryRouter>
            <RightToolbarProvider>
              <ConversationList selectedId={null} onSelect={() => {}} query="" />
            </RightToolbarProvider>
          </MemoryRouter>
        </Providers>,
      );
    });

    const logged = loggedErrors();
    expect(logged).not.toMatch(/Maximum update depth/);
    expect(logged).not.toMatch(/Should have a queue/);
  });

  describe("Select all", () => {
    // jsdom lays nothing out, so the virtual list would draw no rows. A
    // 400-pixel viewport is all TanStack Virtual reads to place them.
    let restoreLayout: (() => void)[] = [];
    beforeEach(() => {
      const viewport = 400;
      restoreLayout = [
        vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(viewport),
        vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(300),
        vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(viewport),
        vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
          top: 0,
          bottom: viewport,
          left: 0,
          right: 300,
          width: 300,
          height: viewport,
        } as DOMRect),
      ].map((spy) => () => spy.mockRestore());
    });
    afterEach(() => {
      for (const restore of restoreLayout) restore();
    });

    function chat(id: number): Conversation {
      return {
        id,
        participants: [],
        message_count: 1,
        first_message_at: "2024-01-01T10:00:00Z",
        last_message_at: "2024-01-01T10:00:00Z",
        service: "text_message",
        is_group: false,
        shown_title: `Chat ${id}`,
        tags: [],
      };
    }

    /** The server's paging over `count` conversations, each carrying `tags`. */
    function serveConversations(count: number, tags: string[] = []) {
      const all = Array.from({ length: count }, (_, i) => ({ ...chat(i + 1), tags }));
      vi.mocked(listConversations).mockImplementation(async ({ limit = 40, offset = 0 }) => ({
        items: all.slice(offset, offset + limit),
        total: all.length,
        limit,
        offset,
      }));
    }

    function renderList() {
      return render(
        <Providers>
          <MemoryRouter>
            <RightToolbarProvider>
              <RightPane>
                <ConversationList selectedId={null} onSelect={() => {}} query="" />
              </RightPane>
            </RightToolbarProvider>
          </MemoryRouter>
        </Providers>,
      );
    }

    it("does not read as all when more conversations are on the server", async () => {
      vi.mocked(listConversations).mockResolvedValue({
        items: [chat(1), chat(2)],
        total: 3000,
        limit: 40,
        offset: 0,
      });
      renderList();
      await settle();
      const user = setupUser();
      await user.click(screen.getByRole("checkbox", { name: "Select Chat 1" }));
      await settle();
      await user.click(screen.getByRole("checkbox", { name: "Select Chat 2" }));
      await settle();

      // Two of 3,000 are ticked.
      const box = screen.getByRole("checkbox", { name: "Select all conversations" });
      expect(box).not.toBeChecked();
    });

    it("tags every conversation the list holds, not only the loaded page", async () => {
      serveConversations(120);
      vi.mocked(updateMessageTagMembers).mockResolvedValue({ added: 120, removed: 0 });
      renderList();
      await settle();
      const user = setupUser();
      expect(screen.getByRole("checkbox", { name: "Select Chat 1" })).toBeInTheDocument();

      const box = screen.getByRole("checkbox", { name: "Select all conversations" });
      await user.click(box);
      await settle();
      expect(box).toBeChecked();

      await user.click(screen.getByRole("button", { name: "Message Tags" }));
      await settle();
      await user.click(screen.getByRole("checkbox", { name: "Holiday" }));
      await settle();

      expect(vi.mocked(updateMessageTagMembers)).toHaveBeenCalled();
      const [id, body] = vi.mocked(updateMessageTagMembers).mock.calls[0] ?? [];
      expect(id).toBe(1);
      expect([...(body?.add ?? [])].sort((a, b) => a - b)).toEqual(
        Array.from({ length: 120 }, (_, i) => i + 1),
      );
    });

    it("clears every Message Tag on the selected conversations at once", async () => {
      serveConversations(2, ["Holiday", "Receipts"]);
      const members = vi.mocked(updateMessageTagMembers);
      members.mockReset();
      // The first write never answers, so Clear all reaches the second tag
      // only if it does not wait for the first.
      members.mockReturnValueOnce(new Promise(() => {}));
      members.mockResolvedValue({ added: 0, removed: 2 });
      renderList();
      await settle();
      const user = setupUser();
      await user.click(screen.getByRole("checkbox", { name: "Select Chat 1" }));
      await settle();
      await user.click(screen.getByRole("checkbox", { name: "Select Chat 2" }));
      await settle();

      await user.click(screen.getByRole("button", { name: "Message Tags" }));
      await settle();
      await user.click(screen.getByRole("button", { name: "Clear all" }));
      await settle();

      expect(members).toHaveBeenCalledTimes(2);
      const writes = members.mock.calls
        .map(([id, body]) => ({ id, remove: [...(body?.remove ?? [])].sort((a, b) => a - b) }))
        .sort((a, b) => a.id - b.id);
      expect(writes).toEqual([
        { id: 1, remove: [1, 2] },
        { id: 2, remove: [1, 2] },
      ]);
    });

    it("shows the server's refusal of a Message Tag created from the Message Tags menu", async () => {
      serveConversations(2);
      const refusal = "name must be at most 80 characters";
      const longName = "Trips ".repeat(14).trim();
      vi.mocked(createMessageTag).mockRejectedValue(new ApiError(422, refusal));
      renderList();
      await settle();
      const user = setupUser();

      await user.click(screen.getByRole("checkbox", { name: "Select Chat 1" }));
      await settle();
      await user.click(screen.getByRole("button", { name: "Message Tags" }));
      await settle();
      await user.click(screen.getByRole("button", { name: /Create Message Tag$/ }));
      await settle();
      await user.type(screen.getByPlaceholderText("Message Tag name"), `${longName}{Enter}`);
      await settle();

      expect(vi.mocked(createMessageTag)).toHaveBeenCalledWith({ name: longName });
      expect(screen.getByText("Name must be at most 80 characters")).toBeInTheDocument();
      expect(screen.getByPlaceholderText("Message Tag name")).toHaveValue(longName);
    });

    it("clears the ticks when the sort changes, so no action reaches part of them", async () => {
      serveConversations(1200);
      renderList();
      await settle();
      const user = setupUser();
      expect(screen.getByRole("checkbox", { name: "Select Chat 1" })).toBeInTheDocument();
      const box = screen.getByRole("checkbox", { name: "Select all conversations" });
      await user.click(box);
      await settle();
      expect(box).toBeChecked();

      await user.click(screen.getByRole("button", { name: /^Sort conversations by/ }));
      await settle();
      await user.click(screen.getByRole("menuitemradio", { name: "Messages" }));
      await settle();

      expect(box).not.toBeChecked();
      expect(box).not.toBePartiallyChecked();
      expect(screen.getByRole("checkbox", { name: "Select Chat 1" })).not.toBeChecked();
    });

    it("keeps every conversation selected when the list reloads after an action", async () => {
      serveConversations(1200);
      vi.mocked(updateMessageTagMembers).mockResolvedValue({ added: 1200, removed: 0 });
      renderList();
      await settle();
      const user = setupUser();
      expect(screen.getByRole("checkbox", { name: "Select Chat 1" })).toBeInTheDocument();

      const box = screen.getByRole("checkbox", { name: "Select all conversations" });
      await user.click(box);
      await settle();
      expect(box).toBeChecked();

      await user.click(screen.getByRole("button", { name: "Message Tags" }));
      await settle();
      await user.click(screen.getByRole("checkbox", { name: "Holiday" }));
      await settle();
      expect(vi.mocked(updateMessageTagMembers)).toHaveBeenCalledTimes(1);
      // Setting a Message Tag reloads the list, every page of it.
      const lastPageReads = () =>
        vi.mocked(listConversations).mock.calls.filter(([params]) => params.offset === 1040).length;
      expect(lastPageReads()).toBe(2);
      expect(box).toBeChecked();

      await user.click(screen.getByRole("checkbox", { name: "Receipts" }));
      await settle();

      expect(vi.mocked(updateMessageTagMembers)).toHaveBeenCalledTimes(2);
      const [id, body] = vi.mocked(updateMessageTagMembers).mock.calls[1] ?? [];
      expect(id).toBe(2);
      expect(body?.add).toHaveLength(1200);
    });
  });
});
