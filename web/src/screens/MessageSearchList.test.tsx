/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MessageSearchSort } from "../lib/messageSearchSort";
import { listMessages } from "../lib/serverApi";
import type { MessageSearch } from "../lib/types";
import { message } from "../test/apiShapes";
import { mockedAuth, Providers } from "../test/providers";
import { setupUser } from "../test/user";
import MessageSearchList from "./MessageSearchList";

vi.mock("../lib/authContext", () => ({ useAuth: () => mockedAuth }));

vi.mock("../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverApi")>()),
  listMessages: vi.fn(),
}));

const listMessagesMock = vi.mocked(listMessages);

// jsdom lays nothing out, so the real list would measure no room for a row.
// Every row is drawn here, which is all these tests need of it.
vi.mock("../components/VirtualList", async () => {
  const { createElement } = await import("react");
  return {
    default: ({
      count,
      renderItem,
    }: {
      count: number;
      renderItem: (index: number) => import("react").ReactNode;
    }) => createElement("div", null, ...Array.from({ length: count }, (_, i) => renderItem(i))),
  };
});

// jsdom has no ResizeObserver; VirtualList observes its scroll container on mount.
class StubResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", StubResizeObserver);
  listMessagesMock.mockReset();
  answer({ sort: "relevance", terms: [{ text: "photo", prefix: false }] });
});

/** The server answers every page with `search`, and 12,408 messages in all. */
function answer(search: MessageSearch, items = [] as ReturnType<typeof message>[]) {
  listMessagesMock.mockResolvedValue({ items, total: 12408, limit: 40, offset: 0, search });
}

afterEach(() => {
  vi.unstubAllGlobals();
  cleanup();
});

function renderList(query: string, sortPick: MessageSearchSort | null = null) {
  const onSortPick = vi.fn();
  render(
    <Providers>
      <MessageSearchList
        query={query}
        sortPick={sortPick}
        onSortPick={onSortPick}
        selectedId={null}
        onSelect={() => {}}
      />
    </Providers>,
  );
  return { onSortPick };
}

/** The labels of the sort menu's choices, Sort By then Order, once it is open. */
async function sortChoices(): Promise<string[]> {
  await setupUser().click(screen.getByRole("button", { name: /^Sort messages by/ }));
  const menu = await screen.findByRole("menu");
  return [...menu.querySelectorAll('[role="menuitemradio"]')].map((item) => item.textContent ?? "");
}

describe("MessageSearchList", () => {
  it("asks for a search, and lists nothing, when the search is empty", () => {
    renderList("  ");
    expect(screen.getByText("Type a search above to list the messages it matches.")).toBeVisible();
    expect(listMessagesMock).not.toHaveBeenCalled();
  });

  it("sends no sort, and shows the order the server applied", async () => {
    renderList("from:Alice photo");
    expect(await screen.findByText("12,408 messages")).toBeVisible();
    expect(listMessagesMock).toHaveBeenCalledWith(
      { q: "from:Alice photo", limit: 40, offset: 0 },
      expect.anything(),
    );
    expect(screen.getByRole("button", { name: "Sort messages by Relevance" })).toBeVisible();
    expect(await sortChoices()).toEqual(["Relevance", "Date"]);
  });

  it("offers only Date when the server returns no terms to rank by", async () => {
    answer({ sort: "-date", terms: [] });
    renderList("from:Alice date:2024");
    expect(
      await screen.findByRole("button", { name: "Sort messages by Date, Newest first" }),
    ).toBeVisible();
    expect(listMessagesMock).toHaveBeenCalledWith(
      { q: "from:Alice date:2024", limit: 40, offset: 0 },
      expect.anything(),
    );
    expect(await sortChoices()).toEqual(["Date", "Oldest first", "Newest first"]);
  });

  it("takes the order and the terms from the server, not from the words typed", async () => {
    // A free-text word the server reports no term for: the menu follows the
    // server, and offers no Relevance.
    answer({ sort: "-date", terms: [] });
    renderList("photo");
    expect(
      await screen.findByRole("button", { name: "Sort messages by Date, Newest first" }),
    ).toBeVisible();
    expect(await sortChoices()).toEqual(["Date", "Oldest first", "Newest first"]);
  });

  it("bolds the terms the server returned", async () => {
    answer({ sort: "relevance", terms: [{ text: "dent", prefix: true }] }, [
      message({ id: 7, text: "Here is the photo from the dentist" }),
    ]);
    renderList("whatever was typed");
    const row = await screen.findByRole("button", { name: /photo from the dentist/ });
    expect([...row.querySelectorAll("strong")].map((b) => b.textContent)).toEqual(["dentist"]);
  });

  it("sends a picked date order, and hands a new pick to the caller", async () => {
    answer({ sort: "date", terms: [{ text: "photo", prefix: false }] });
    const { onSortPick } = renderList("photo", { sort: "date", order: "asc" });
    await waitFor(() =>
      expect(listMessagesMock).toHaveBeenCalledWith(
        expect.objectContaining({ sort: "date" }),
        expect.anything(),
      ),
    );
    const user = setupUser();
    await user.click(
      await screen.findByRole("button", { name: "Sort messages by Date, Oldest first" }),
    );
    await user.click(screen.getByRole("menuitemradio", { name: "Relevance" }));
    expect(onSortPick).toHaveBeenCalledWith({ sort: "relevance", order: "asc" });
  });
});
