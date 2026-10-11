/** @vitest-environment jsdom */

import { act, cleanup, render, screen } from "@testing-library/react";
import { type ReactNode, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { setupUser } from "../test/user";
import {
  AppLayoutContext,
  type NavItem,
  useLayoutNavItem,
  useLayoutSection,
  useSectionSlot,
} from "./appLayoutContext";

afterEach(cleanup);

let layoutRenders = 0;

/** A layout that shows the declared section and offers the header's search. */
function Layout({ children }: { children: ReactNode }) {
  layoutRenders += 1;
  const { shown, declareSection, navItem, declareNavItem, onSearch } = useSectionSlot();
  return (
    <AppLayoutContext.Provider
      value={{
        declareSection,
        declareNavItem,
        selectedContact: null,
        selectContact: () => {},
        closeContactDrawer: () => {},
        browseContactConversations: () => {},
      }}
    >
      <output data-testid="shown">
        {shown ? `${shown.search.target}:${shown.search.query}:${shown.browseQuery}` : "none"}
      </output>
      <output data-testid="nav-item">{navItem ?? "none"}</output>
      <button type="button" onClick={() => onSearch("ada")}>
        Search
      </button>
      {children}
    </AppLayoutContext.Provider>
  );
}

/** A route with a list: it searches contacts for `query`, and reports a search to `onSearch`. */
function ListRoute({ query, onSearch }: { query: string; onSearch: (q: string) => void }) {
  useLayoutSection({
    search: { target: "contacts", query },
    onSearchChange: () => {},
    onSearch,
    browseQuery: "",
    navItem: "contacts",
  });
  return null;
}

/** A route with no list, on the navigation panel's `item`. */
function NoListTestRoute({ item }: { item: NavItem | null }) {
  useLayoutNavItem(item);
  return <p>No list</p>;
}

/** Two routes, switched by a button, as the router would switch them. */
function Routes({ noListItem = null }: { noListItem?: NavItem | null }) {
  const [onList, setOnList] = useState(true);
  return (
    <Layout>
      <button type="button" onClick={() => setOnList((v) => !v)}>
        Switch
      </button>
      {onList ? (
        <ListRoute query="bob" onSearch={() => {}} />
      ) : (
        <NoListTestRoute item={noListItem} />
      )}
    </Layout>
  );
}

describe("useLayoutSection", () => {
  it("shows the route's section, and nothing once the person leaves for a route with no list", async () => {
    const user = setupUser();
    render(<Routes />);
    expect(screen.getByTestId("shown").textContent).toBe("contacts:bob:");

    // A route left behind must not leave its search in the header.
    await user.click(screen.getByRole("button", { name: "Switch" }));
    expect(screen.getByTestId("shown").textContent).toBe("none");

    await user.click(screen.getByRole("button", { name: "Switch" }));
    expect(screen.getByTestId("shown").textContent).toBe("contacts:bob:");
  });

  it("hands the navigation panel the item of the route the person is on", async () => {
    const user = setupUser();
    const { unmount } = render(<Routes noListItem="import" />);
    expect(screen.getByTestId("nav-item").textContent).toBe("contacts");

    // The route left behind takes its item with it, and the next one's stays.
    await user.click(screen.getByRole("button", { name: "Switch" }));
    expect(screen.getByTestId("nav-item").textContent).toBe("import");
    await user.click(screen.getByRole("button", { name: "Switch" }));
    expect(screen.getByTestId("nav-item").textContent).toBe("contacts");
    unmount();

    // A route the panel has no item for, such as Settings, highlights nothing.
    render(<Routes />);
    await user.click(screen.getByRole("button", { name: "Switch" }));
    expect(screen.getByTestId("nav-item").textContent).toBe("none");
  });

  it("hands the header's search to the route's newest callback", async () => {
    const first = vi.fn();
    const second = vi.fn();
    const user = setupUser();
    const { rerender } = render(
      <Layout>
        <ListRoute query="bob" onSearch={first} />
      </Layout>,
    );
    // Same words, new callback: the layout must still call the new one, or a
    // search would act on an address the route has already left.
    rerender(
      <Layout>
        <ListRoute query="bob" onSearch={second} />
      </Layout>,
    );

    await user.click(screen.getByRole("button", { name: "Search" }));
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith("ada");
  });

  it("does not render the layout again when the route renders again with the same words", () => {
    const { rerender } = render(
      <Layout>
        <ListRoute query="bob" onSearch={() => {}} />
      </Layout>,
    );
    // Without this, every render of the route would render the layout, and
    // the layout renders the route: a loop.
    const before = layoutRenders;
    act(() => {
      rerender(
        <Layout>
          <ListRoute query="bob" onSearch={() => {}} />
        </Layout>,
      );
    });
    expect(layoutRenders - before).toBe(1);
  });
});
