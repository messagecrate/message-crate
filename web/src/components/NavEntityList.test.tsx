/** @vitest-environment jsdom */

/**
 * Delete on `NavEntityList` asks first, and `rename` and `remove` navigate away from the renamed or
 * deleted item's own page — to the new slug, or to the collection's fallback
 * route. The server's write routes are faked by name, the way
 * `useNameCollectionActions.test.tsx` fakes them, so this never touches a URL string
 * except the one under test.
 */

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { authModule, Providers } from "../test/providers";
import { fill, setupUser } from "../test/user";
import GroupsNav from "./GroupsNav";
import MessageTagsNav from "./MessageTagsNav";

vi.mock("../hooks/useAuth", () => authModule());

const routes = vi.hoisted(() => ({
  listContactGroups: vi.fn(),
  updateContactGroup: vi.fn(),
  deleteContactGroup: vi.fn(),
  listMessageTags: vi.fn(),
  updateMessageTag: vi.fn(),
  deleteMessageTag: vi.fn(),
}));

vi.mock("../lib/serverApi", () => ({
  listContactGroups: routes.listContactGroups,
  createContactGroup: vi.fn(),
  updateContactGroup: routes.updateContactGroup,
  deleteContactGroup: routes.deleteContactGroup,
  updateContactGroupMembers: vi.fn(),
  listMessageTags: routes.listMessageTags,
  createMessageTag: vi.fn(),
  updateMessageTag: routes.updateMessageTag,
  deleteMessageTag: routes.deleteMessageTag,
  updateMessageTagMembers: vi.fn(),
}));

beforeEach(() => {
  startFakeClock();
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  stopFakeClock();
});

/** Renders the current path so a test can read where `navigate` landed. */
function LocationDisplay() {
  const location = useLocation();
  return <div data-testid="location">{location.pathname}</div>;
}

function renderGroups(path: string, groups: string[]) {
  routes.listContactGroups.mockResolvedValue(groups.map((name, i) => ({ id: i + 1, name })));
  return render(
    <Providers>
      <MemoryRouter initialEntries={[path]}>
        <LocationDisplay />
        <GroupsNav groups={groups} />
      </MemoryRouter>
    </Providers>,
  );
}

function renderTags(path: string, tags: string[]) {
  routes.listMessageTags.mockResolvedValue(tags.map((name, i) => ({ id: i + 1, name })));
  return render(
    <Providers>
      <MemoryRouter initialEntries={[path]}>
        <LocationDisplay />
        <MessageTagsNav tags={tags} />
      </MemoryRouter>
    </Providers>,
  );
}

describe("NavEntityList row menu", () => {
  it("closes the row menu when its options button is clicked again", async () => {
    const user = setupUser();
    renderGroups("/contacts", ["Family"]);

    const options = screen.getByRole("button", { name: "Contact Group options for Family" });
    await user.click(options);
    expect(
      screen.getByRole("menu", { name: "Contact Group options for Family" }),
    ).toBeInTheDocument();

    await user.click(options);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});

describe("NavEntityList current row", () => {
  /** The rows whose button says it is the current page, by name. */
  function currentRows(): string[] {
    return screen
      .getAllByRole("button")
      .filter((button) => button.getAttribute("aria-current") === "page")
      .map((button) => button.textContent ?? "");
  }

  it.each([
    ["/group/Family", "Family"],
    // The page opens a set whose slug differs only in case, so its row is current too.
    ["/group/family", "Family"],
    ["/unknown", "Unknown"],
    ["/no-group", "No Contact Group"],
  ])("marks the open Contact Group row at %s, and no other", (path, row) => {
    renderGroups(path, ["Family", "Work"]);
    expect(currentRows()).toEqual([row]);
  });

  it.each([
    ["/tag/Receipts", "Receipts"],
    ["/tag/receipts", "Receipts"],
    ["/no-tag", "No Message Tag"],
  ])("marks the open Message Tag row at %s, and no other", (path, row) => {
    renderTags(path, ["Receipts", "Travel"]);
    expect(currentRows()).toEqual([row]);
  });

  it("marks no row when no set is open", () => {
    renderGroups("/contacts", ["Family", "Work"]);
    expect(currentRows()).toEqual([]);
  });
});

describe("NavEntityList navigation", () => {
  it("follows a renamed group to its new slug when viewing that group's page", async () => {
    routes.updateContactGroup.mockResolvedValue({ id: 1, name: "Fam" });
    const user = setupUser();
    renderGroups("/group/Family", ["Family"]);

    await user.click(screen.getByRole("button", { name: "Contact Group options for Family" }));
    await user.click(screen.getByRole("menuitem", { name: "Rename…" }));
    const input = screen.getByPlaceholderText("Contact Group name");
    await user.clear(input);
    await fill(user, input, "Fam");
    await user.click(screen.getByRole("button", { name: "Save" }));

    // The page moves once the rename is written and the cache has read the
    // lists again, a few renders after Save, all of which `settle` lands.
    await settle();
    expect(screen.getByTestId("location").textContent).toBe("/group/Fam");
  });

  it("falls back to the group collection's home route after deleting the group being viewed", async () => {
    routes.deleteContactGroup.mockResolvedValue(undefined);
    const user = setupUser();
    renderGroups("/group/Family", ["Family"]);

    await user.click(screen.getByRole("button", { name: "Contact Group options for Family" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));

    await settle();
    expect(screen.getByTestId("location").textContent).toBe("/contacts");
  });

  it("falls back after deleting the group being viewed at a slug in another case", async () => {
    routes.deleteContactGroup.mockResolvedValue(undefined);
    const user = setupUser();
    renderGroups("/group/family", ["Family"]);

    await user.click(screen.getByRole("button", { name: "Contact Group options for Family" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));

    await settle();
    expect(screen.getByTestId("location").textContent).toBe("/contacts");
  });

  it("follows a renamed tag to its new slug when viewing that tag's page", async () => {
    routes.updateMessageTag.mockResolvedValue({ id: 1, name: "Vacation" });
    const user = setupUser();
    renderTags("/tag/Holiday", ["Holiday"]);

    await user.click(screen.getByRole("button", { name: "Message Tag options for Holiday" }));
    await user.click(screen.getByRole("menuitem", { name: "Rename…" }));
    const input = screen.getByPlaceholderText("Message Tag name");
    await user.clear(input);
    await fill(user, input, "Vacation");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await settle();
    expect(screen.getByTestId("location").textContent).toBe("/tag/Vacation");
  });

  it("falls back to the tag collection's home route after deleting the tag being viewed", async () => {
    routes.deleteMessageTag.mockResolvedValue(undefined);
    const user = setupUser();
    renderTags("/tag/Holiday", ["Holiday"]);

    await user.click(screen.getByRole("button", { name: "Message Tag options for Holiday" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));

    await settle();
    expect(screen.getByTestId("location").textContent).toBe("/");
  });

  it("asks before deleting a group, and says what goes and what stays", async () => {
    const user = setupUser();
    renderGroups("/group/Family", ["Family"]);

    await user.click(screen.getByRole("button", { name: "Contact Group options for Family" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));

    const dialog = screen.getByRole("dialog", { name: "Delete Family?" });
    expect(dialog).toHaveTextContent(
      "Removes the Contact Group Family and takes every contact out of it. The contacts themselves stay in your Message Crate.",
    );
    expect(routes.deleteContactGroup).not.toHaveBeenCalled();
  });

  it("deletes nothing when the confirmation is canceled", async () => {
    const user = setupUser();
    renderTags("/tag/Holiday", ["Holiday"]);

    await user.click(screen.getByRole("button", { name: "Message Tag options for Holiday" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete Holiday?" })).toHaveTextContent(
      "Removes the Message Tag Holiday and takes it off every conversation that carries it. The conversations themselves stay in your Message Crate.",
    );
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(routes.deleteMessageTag).not.toHaveBeenCalled();
    expect(screen.getByTestId("location").textContent).toBe("/tag/Holiday");
  });

  it("deletes the group once the confirmation is confirmed", async () => {
    routes.deleteContactGroup.mockResolvedValue(undefined);
    const user = setupUser();
    renderGroups("/contacts", ["Family"]);

    await user.click(screen.getByRole("button", { name: "Contact Group options for Family" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));

    await settle();
    expect(routes.deleteContactGroup).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("keeps the confirmation open with the server's message when the delete is refused", async () => {
    routes.deleteContactGroup.mockRejectedValue(new Error("contact group not found"));
    const user = setupUser();
    renderGroups("/group/Family", ["Family"]);

    await user.click(screen.getByRole("button", { name: "Contact Group options for Family" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("Contact group not found");
    expect(screen.getByRole("dialog", { name: "Delete Family?" })).toBeTruthy();
    expect(screen.getByTestId("location").textContent).toBe("/group/Family");
  });

  it("links each group by its whole name, so names that differ only in punctuation open their own page", async () => {
    const user = setupUser();
    renderGroups("/contacts", ["A&B", "A B", "家族"]);

    await user.click(screen.getByRole("button", { name: "A B" }));
    expect(screen.getByTestId("location").textContent).toBe("/group/A%20B");
    await user.click(screen.getByRole("button", { name: "A&B" }));
    expect(screen.getByTestId("location").textContent).toBe("/group/A%26B");
    await user.click(screen.getByRole("button", { name: "家族" }));
    expect(screen.getByTestId("location").textContent).toBe(`/group/${encodeURIComponent("家族")}`);
  });

  it("follows a renamed group whose name holds a space when viewing that group's page", async () => {
    routes.updateContactGroup.mockResolvedValue({ id: 1, name: "Old Friends" });
    const user = setupUser();
    renderGroups("/group/Work%20Friends", ["Work Friends"]);

    await user.click(
      screen.getByRole("button", { name: "Contact Group options for Work Friends" }),
    );
    await user.click(screen.getByRole("menuitem", { name: "Rename…" }));
    const input = screen.getByPlaceholderText("Contact Group name");
    await user.clear(input);
    await fill(user, input, "Old Friends");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await settle();
    expect(screen.getByTestId("location").textContent).toBe("/group/Old%20Friends");
  });
});
