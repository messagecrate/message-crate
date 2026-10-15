/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createContactGroup,
  getAccountProfile,
  listContactGroups,
  trashConversation,
  updateContactGroupMembers,
} from "../../lib/serverApi";
import type { Conversation } from "../../lib/types";
import { participant } from "../../test/apiShapes";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { nth } from "../../test/nth";
import { mockedAuth, Providers } from "../../test/providers";
import { fill, setupUser } from "../../test/user";
import ConversationHeader from "./ConversationHeader";

vi.mock("../../lib/authContext", () => ({ useAuth: () => mockedAuth }));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  trashConversation: vi.fn(),
  getAccountProfile: vi.fn(),
  listContactGroups: vi.fn(),
  createContactGroup: vi.fn(),
  updateContactGroupMembers: vi.fn(),
}));

const trashConversationMock = vi.mocked(trashConversation);
const getAccountProfileMock = vi.mocked(getAccountProfile);
const listContactGroupsMock = vi.mocked(listContactGroups);
const createContactGroupMock = vi.mocked(createContactGroup);
const updateContactGroupMembersMock = vi.mocked(updateContactGroupMembers);

/** The logged-in account: one phone, so the owner can be told apart from the others. */
const PROFILE = {
  account_id: 7,
  username: "me",
  app: null,
  app_build: null,
  last_login_at: null,
  preferred_name: "Me",
  time_zone: "UTC",
  phones: [{ address: "+15550100", services: ["phone" as const] }],
  emails: [],
  is_owner: false,
  disabled: false,
  must_set_up_profile: false,
  has_password: true,
  is_demo: false,
  can_import: true,
  can_export: true,
  can_delete: true,
  message_count: 0,
  storage_bytes: 0,
};

/** A group conversation: the owner, two people with contacts, one nobody has a contact for. */
function groupConversation(): Conversation {
  return conversation({
    is_group: true,
    shown_title: "Book Club",
    participants: [
      participant({ name: "Me", identity: "+1 (555) 010-0", contact_id: 1 }),
      participant({ name: "Ada", identity: "+15550120", contact_id: 2 }),
      participant({ name: "Grace", identity: "+15550130", contact_id: 3 }),
      participant({ name: "+15550140", identity: "+15550140" }),
    ],
  });
}

function conversation(overrides: Partial<Conversation> = {}): Conversation {
  return {
    id: 42,
    participants: [],
    message_count: 3,
    first_message_at: "2024-01-01T10:00:00Z",
    last_message_at: "2024-01-01T10:00:00Z",
    service: "sms",
    is_group: false,
    shown_title: "Chat 42",
    tags: [],
    ...overrides,
  };
}

function renderHeader(
  c: Conversation,
  {
    years = [],
    onJumpToNewest = () => {},
    onJumpToYear = () => {},
    displayParticipants = [],
    onOpenContact,
  }: {
    years?: number[];
    onJumpToNewest?: () => void;
    onJumpToYear?: (year: number) => void;
    displayParticipants?: { label: string; contact_id?: string | null }[];
    onOpenContact?: (contactId: string) => void;
  } = {},
) {
  return render(
    <Providers>
      <MemoryRouter initialEntries={["/messages/42"]}>
        <Routes>
          <Route
            path="/messages/:id"
            element={
              <ConversationHeader
                conversation={c}
                displayParticipants={displayParticipants}
                onOpenContact={onOpenContact}
                years={years}
                findOpen={false}
                onToggleFind={() => {}}
                onJumpToNewest={onJumpToNewest}
                onJumpToYear={onJumpToYear}
                onShowSources={() => {}}
              />
            }
          />
          <Route path="/" element={<div>Conversations list</div>} />
        </Routes>
      </MemoryRouter>
    </Providers>,
  );
}

/**
 * Open the ⋯ menu and choose an item. An item that waits on a fetch is there
 * only once the test has settled the fetch's answer.
 */
async function openMenuItem(user: ReturnType<typeof setupUser>, name: string) {
  await user.click(screen.getByRole("button", { name: "More for this conversation" }));
  await user.click(screen.getByRole("menuitem", { name }));
}

describe("ConversationHeader", () => {
  beforeEach(() => {
    startFakeClock();
    trashConversationMock.mockReset();
    getAccountProfileMock.mockReset();
    listContactGroupsMock.mockReset();
    createContactGroupMock.mockReset();
    updateContactGroupMembersMock.mockReset();
    getAccountProfileMock.mockResolvedValue(PROFILE);
    listContactGroupsMock.mockResolvedValue([]);
  });

  afterEach(() => {
    cleanup();
    stopFakeClock();
  });

  describe("Make a Contact Group", () => {
    it("is offered on a group conversation and not on a one-to-one conversation", async () => {
      const user = setupUser();
      // The same people as the group conversation, so only the conversation's
      // kind keeps the item off the menu.
      renderHeader(conversation({ participants: groupConversation().participants }));
      // The profile the item waits for has landed, so its absence is the screen's choice.
      await settle();
      await user.click(screen.getByRole("button", { name: "More for this conversation" }));
      expect(screen.queryByRole("menuitem", { name: "Make a Contact Group" })).toBeNull();
      cleanup();

      renderHeader(groupConversation());
      // The owner's profile decides who is left out, so the item waits for it.
      await settle();
      expect(getAccountProfileMock).toHaveBeenCalled();
      await openMenuItem(user, "Make a Contact Group");
    });

    it("creates the group and adds everyone but the owner and the contact-less", async () => {
      // The server, modelled: once created, the group is in the list the
      // members call looks the id up in.
      let groups: { id: number; name: string }[] = [];
      listContactGroupsMock.mockImplementation(async () => groups);
      createContactGroupMock.mockImplementation(async ({ name }) => {
        const set = { id: 9, name };
        groups = [set];
        return set;
      });
      updateContactGroupMembersMock.mockResolvedValue({ added: 2, removed: 0 });
      const user = setupUser();
      renderHeader(groupConversation());
      await settle();

      await openMenuItem(user, "Make a Contact Group");
      // The chat's label is offered as the name.
      const input = screen.getByDisplayValue("Book Club");
      await user.clear(input);
      await fill(user, input, "Readers");
      await user.click(screen.getByRole("button", { name: "Create" }));

      await settle();
      expect(createContactGroupMock).toHaveBeenCalledWith({ name: "Readers" });
      expect(updateContactGroupMembersMock).toHaveBeenCalledWith(9, {
        add: [2, 3],
        remove: [],
      });
      expect(screen.getByText("Added 2 people to Readers.")).toBeTruthy();
    });

    it("adds to an existing group of that name instead of creating a second one", async () => {
      listContactGroupsMock.mockResolvedValue([{ id: 4, name: "Readers" }]);
      updateContactGroupMembersMock.mockResolvedValue({ added: 2, removed: 0 });
      const user = setupUser();
      renderHeader(groupConversation());
      await settle();

      await openMenuItem(user, "Make a Contact Group");
      const input = screen.getByDisplayValue("Book Club");
      await user.clear(input);
      await fill(user, input, "readers");
      await user.click(screen.getByRole("button", { name: "Create" }));

      await settle();
      expect(updateContactGroupMembersMock).toHaveBeenCalledWith(4, {
        add: [2, 3],
        remove: [],
      });
      expect(createContactGroupMock).not.toHaveBeenCalled();
    });
  });

  it("says in one line who is in it, the service as the conversation list names it, and how many messages", () => {
    renderHeader(groupConversation());
    expect(screen.getByRole("heading", { name: "Book Club" })).toBeInTheDocument();
    expect(screen.getByText("4 people")).toBeInTheDocument();
    // `sms` reads "Text Message", the conversation list's word, not the raw service.
    expect(screen.getByText("Text Message")).toBeInTheDocument();
    expect(screen.getByText("3 messages")).toBeInTheDocument();
  });

  it("offers Newest and every year, newest first, under Jump to", async () => {
    const onJumpToNewest = vi.fn();
    const onJumpToYear = vi.fn();
    const user = setupUser();
    renderHeader(conversation(), { years: [2021, 2022, 2023], onJumpToNewest, onJumpToYear });

    await user.click(screen.getByRole("button", { name: "Jump to ▾" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Newest",
      "2023",
      "2022",
      "2021",
    ]);
    await user.click(screen.getByRole("menuitem", { name: "2021" }));
    expect(onJumpToYear).toHaveBeenCalledWith(2021);

    await user.click(screen.getByRole("button", { name: "Jump to ▾" }));
    await user.click(screen.getByRole("menuitem", { name: "Newest" }));
    expect(onJumpToNewest).toHaveBeenCalled();
  });

  it("lists a person named like a fixed row beside that row, and opens them", async () => {
    const onOpenContact = vi.fn();
    const user = setupUser();
    renderHeader(conversation(), {
      displayParticipants: [{ label: "Sources", contact_id: "7" }],
      onOpenContact,
    });

    await user.click(screen.getByRole("button", { name: "More for this conversation" }));
    const rows = screen.getAllByRole("menuitem", { name: "Sources" });
    expect(rows).toHaveLength(2);
    await user.click(nth(rows, 0));

    expect(onOpenContact).toHaveBeenCalledWith("7");
  });

  it("moves the conversation to trash and navigates back to the conversations list", async () => {
    trashConversationMock.mockResolvedValue(undefined);
    const user = setupUser();
    renderHeader(conversation());

    await openMenuItem(user, "Move to trash");

    expect(trashConversationMock).toHaveBeenCalledWith(42, expect.anything());
    await settle();
    expect(screen.getByText("Conversations list")).toBeInTheDocument();
  });

  it("shows an error and stays put when trashing fails", async () => {
    trashConversationMock.mockRejectedValue(new Error("Could not move this conversation."));
    const user = setupUser();
    renderHeader(conversation());

    await openMenuItem(user, "Move to trash");
    await settle();

    expect(screen.getByText("Could not move this conversation.")).toBeInTheDocument();
    expect(screen.queryByText("Conversations list")).not.toBeInTheDocument();
  });
});
