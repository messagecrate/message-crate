/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { fill, setupUser } from "../test/user";
import AppHeader from "./AppHeader";

vi.mock("./AppAccountMenu", () => ({ default: () => null }));
vi.mock("./VersionNotice", () => ({ default: () => null }));
vi.mock("../lib/useSearchSuggestions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/useSearchSuggestions")>()),
  useSearchSuggestions: () => [],
}));

vi.mock("../lib/searchFields", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/searchFields")>()),
  useMarkedWords: () => ({ marked: [], ready: true }),
}));

afterEach(cleanup);

const props = {
  search: { target: "conversations" as const, query: "" },
  onSearchChange: vi.fn(),
  onSearch: vi.fn(),
};

describe("AppHeader", () => {
  it("shows no search box on a screen with nothing to search", () => {
    // Import, Export and Settings have no list (#1568).
    render(<AppHeader {...props} search={null} />);

    expect(screen.getByText("Message Crate")).toBeTruthy();
    expect(screen.queryByRole("combobox")).toBeNull();
  });

  it("starts the search box again when a list comes back", async () => {
    // A screen with no list takes the box away, so text typed in it before
    // does not come back with it; the box shows the list's search.
    const user = setupUser();
    const { rerender } = render(<AppHeader {...props} />);
    await fill(user, screen.getByRole("combobox", { name: "Search conversations" }), "foo");

    rerender(<AppHeader {...props} search={null} />);
    rerender(<AppHeader {...props} />);

    expect(screen.getByRole("combobox", { name: "Search conversations" })).toHaveValue("");
  });
});
