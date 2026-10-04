/** @vitest-environment jsdom */

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../lib/api";
import { CONTACT_GROUP_MENU_COPY, MESSAGE_TAG_MENU_COPY } from "../lib/namedSetCopy";
import { setupUser } from "../test/user";
import GroupsMenu from "./GroupsMenu";

afterEach(() => {
  cleanup();
});

const GROUPS = ["College", "Family", "Work"] as const;
const ROW_TOKENS = ["px-3", "py-1.5", "text-[0.813rem]", "leading-5"] as const;

function renderMenu(labeled = true) {
  return render(
    <GroupsMenu
      allGroups={[...GROUPS]}
      checks={{ College: "on", Family: "off", Work: "off" }}
      labeled={labeled}
      copy={labeled ? CONTACT_GROUP_MENU_COPY : MESSAGE_TAG_MENU_COPY}
    />,
  );
}

describe("GroupsMenu", () => {
  it("filters labeled group names as the user types", async () => {
    const user = setupUser();
    renderMenu();

    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    expect(screen.getByText("College")).toBeTruthy();
    expect(screen.getByText("Family")).toBeTruthy();
    expect(screen.getByText("Work")).toBeTruthy();

    await user.type(screen.getByRole("searchbox", { name: "Search Contact Groups…" }), "fam");
    expect(screen.queryByText("College")).toBeNull();
    expect(screen.getByText("Family")).toBeTruthy();
    expect(screen.queryByText("Work")).toBeNull();
  });

  it("keeps the empty message on the same row metrics as a group row", async () => {
    const user = setupUser();
    renderMenu();

    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    const groupRow = screen.getByText("Family").closest("label");
    expect(groupRow).toBeTruthy();
    for (const token of ROW_TOKENS) {
      expect(groupRow?.className).toContain(token);
    }

    await user.type(screen.getByRole("searchbox", { name: "Search Contact Groups…" }), "zzz");
    const empty = screen.getByRole("status");
    expect(empty.tagName).toBe("DIV");
    expect(empty.textContent).toContain("No matching Contact Groups");
    for (const token of ROW_TOKENS) {
      expect(empty.className).toContain(token);
    }
    expect(screen.queryByText("No Contact Groups")).toBeNull();
  });

  it("shows no groups on the same row when the catalog is empty", async () => {
    const user = setupUser();
    render(<GroupsMenu allGroups={[]} checks={{}} labeled />);

    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    const empty = screen.getByRole("status");
    expect(empty.textContent).toContain("No Contact Groups");
    for (const token of ROW_TOKENS) {
      expect(empty.className).toContain(token);
    }
    expect(screen.queryByText("No matching Contact Groups")).toBeNull();
  });

  it("filters the icon-only tags menu", async () => {
    const user = setupUser();
    renderMenu(false);

    await user.click(screen.getByRole("button", { name: "Message Tags" }));
    await user.type(screen.getByRole("searchbox", { name: "Search Message Tags…" }), "wor");
    expect(screen.getByText("Work")).toBeTruthy();
    expect(screen.queryByText("College")).toBeNull();
    expect(screen.queryByText("Family")).toBeNull();
  });

  it("shows the server's refusal of a new name and keeps the typed name", async () => {
    const user = setupUser();
    const refusal =
      'name can\'t hold ";", because the address book separates Contact Group names with it';
    const onCreate = vi.fn().mockRejectedValue(new ApiError(422, refusal));
    render(<GroupsMenu allGroups={[...GROUPS]} checks={{}} onCreate={onCreate} />);

    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    await user.click(screen.getByRole("button", { name: /Create Contact Group$/ }));
    await user.type(screen.getByPlaceholderText("Contact Group name"), "Work; 2024");
    await user.click(screen.getByRole("button", { name: "Create" }));

    expect(onCreate).toHaveBeenCalledWith("Work; 2024");
    expect(await screen.findByText(refusal)).toBeTruthy();
    expect(screen.getByPlaceholderText("Contact Group name")).toHaveValue("Work; 2024");
  });

  it("returns to the list once the new name is created", async () => {
    const user = setupUser();
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(<GroupsMenu allGroups={[...GROUPS]} checks={{}} onCreate={onCreate} />);

    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    await user.click(screen.getByRole("button", { name: /Create Contact Group$/ }));
    await user.type(screen.getByPlaceholderText("Contact Group name"), "Friends{Enter}");

    expect(onCreate).toHaveBeenCalledWith("Friends");
    expect(await screen.findByRole("searchbox", { name: "Search Contact Groups…" })).toBeTruthy();
    expect(screen.queryByPlaceholderText("Contact Group name")).toBeNull();
  });

  it("leaves a new form alone when a create from a closed form settles", async () => {
    const user = setupUser();
    let refuse!: (error: Error) => void;
    const onCreate = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise<void>((_, reject) => {
            refuse = reject;
          }),
      )
      .mockResolvedValue(undefined);
    render(<GroupsMenu allGroups={[...GROUPS]} checks={{}} onCreate={onCreate} />);

    // "A" goes out, then the menu is dismissed before the server answers.
    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    await user.click(screen.getByRole("button", { name: /Create Contact Group$/ }));
    await user.type(screen.getByPlaceholderText("Contact Group name"), "A{Enter}");
    await user.keyboard("{Escape}");

    // A new form, with "B" typed and Create ready to press.
    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    await user.click(screen.getByRole("button", { name: /Create Contact Group$/ }));
    await user.type(screen.getByPlaceholderText("Contact Group name"), "B");
    expect(screen.getByRole("button", { name: "Create" })).toBeEnabled();

    await act(async () => refuse(new ApiError(422, "name must be at most 80 characters")));
    expect(screen.queryByText("name must be at most 80 characters")).toBeNull();
    expect(screen.getByPlaceholderText("Contact Group name")).toHaveValue("B");
  });
});
