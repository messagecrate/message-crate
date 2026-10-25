/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../../lib/api";
import { getContact } from "../../lib/serverApi";
import type { components } from "../../lib/serverApi.types";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { Providers } from "../../test/providers";
import { setupUser } from "../../test/user";
import MergeContactsDialog, { type MergeCandidate } from "./MergeContactsDialog";

vi.mock("../../hooks/useAuth", async () =>
  (await import("../../test/providers")).authModule(() => ({ accountId: 7 })),
);
vi.mock("../../lib/serverApi", () => ({ getContact: vi.fn() }));

type Contact = components["schemas"]["Contact"];
type Identity = components["schemas"]["Identity"];

function identity(address: string, identity_type: string, service: string): Identity {
  return {
    address,
    identity_type,
    service,
    country_unknown: false,
    start_date: null,
    end_date: null,
    conversations: 1,
    one_to_one_messages: 0,
    group_messages: 0,
    orphaned_messages: 0,
  };
}

const DETAILS: Record<number, Contact> = {
  1: detail(1, "Nicholas Koch", 15459, [identity("+12155550112", "phone", "text_message")]),
  2: detail(2, "Nick Koch", 212, [identity("nick.koch@example.com", "email", "email")]),
  3: detail(3, "", 340, [identity("+12155550113", "phone", "whatsapp")]),
  4: detail(4, "", 20, [identity("+12155550114", "phone", "text_message")]),
};

function detail(id: number, name: string, total: number, identities: Identity[]): Contact {
  return {
    id,
    name,
    unknown: !name,
    identities,
    one_to_one_conversations: 1,
    group_conversations: 0,
    orphaned_conversations: 0,
    total_messages: total,
    last_modified: "2026-01-01 00:00:00",
    groups: [],
  };
}

function candidate(id: number): MergeCandidate {
  const d = DETAILS[id] as Contact;
  return { id, name: d.name, addresses: d.identities.map((i) => i.address) };
}

function renderDialog(ids: number[], onMerge = vi.fn().mockResolvedValue(null)) {
  render(
    <Providers>
      <MergeContactsDialog
        open
        contacts={ids.map(candidate)}
        busy={false}
        onMerge={onMerge}
        onClose={vi.fn()}
      />
    </Providers>,
  );
  return onMerge;
}

const mergeButton = () => screen.getByRole("button", { name: "Merge" });
const nameButton = () => screen.getByRole("button", { name: /Name to keep/ });

beforeEach(() => {
  startFakeClock();
  vi.mocked(getContact).mockImplementation(async (id) => DETAILS[Number(id)] as Contact);
});

afterEach(() => {
  cleanup();
  stopFakeClock();
  vi.clearAllMocks();
});

describe("MergeContactsDialog", () => {
  it("lists every selected contact with its messages, and the nameless ones cannot be chosen", async () => {
    const user = setupUser();
    const onMerge = renderDialog([1, 2, 3]);
    await settle();
    expect(screen.getByRole("heading", { name: "Merge 3 contacts" })).toBeInTheDocument();
    expect(screen.getByText("Choose the name the merged contact keeps.")).toBeInTheDocument();
    expect(mergeButton()).toBeDisabled();

    await user.click(nameButton());
    const options = screen.getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual([
      "Nicholas Koch15,459 messages",
      "Nick Koch212 messages",
      "+12155550113340 messages",
    ]);
    expect(options[2]).toHaveAttribute("aria-disabled", "true");
    expect(within(options[2] as HTMLElement).getByText("+12155550113").tagName).toBe("EM");

    await user.click(screen.getByRole("option", { name: /Nick Koch/ }));
    await user.click(mergeButton());
    await settle();
    expect(onMerge).toHaveBeenCalledWith(2);
  });

  it("lists every identity that moves, the kept contact's first", async () => {
    const user = setupUser();
    renderDialog([1, 2, 3]);
    await settle();
    await user.click(nameButton());
    await user.click(screen.getByRole("option", { name: /Nick Koch/ }));

    const rows = within(screen.getByRole("table")).getAllByRole("row");
    expect(rows.map((r) => [...r.children].map((c) => c.textContent))).toEqual([
      ["From", "Identity", "Type", "Service"],
      ["Nick Koch", "nick.koch@example.com", "Email", "Email"],
      ["Nicholas Koch", "+12155550112", "Phone", "Text Message"],
      ["+12155550113", "+12155550113", "Phone", "WhatsApp"],
    ]);
    expect(screen.queryByText(/conversation/i)).toBeNull();
  });

  it("chooses the one name there is", async () => {
    const user = setupUser();
    const onMerge = renderDialog([3, 1]);
    await settle();
    expect(nameButton()).toHaveTextContent("Nicholas Koch");
    await user.click(mergeButton());
    await settle();
    expect(onMerge).toHaveBeenCalledWith(1);
  });

  it("with no name to choose, reads (no name) and keeps the contact with the most messages", async () => {
    const user = setupUser();
    const onMerge = renderDialog([4, 3]);
    await settle();
    expect(nameButton()).toHaveTextContent("(no name)");
    await user.click(mergeButton());
    await settle();
    expect(onMerge).toHaveBeenCalledWith(3);
  });

  it("stays open with the server's reason when the merge is refused", async () => {
    const user = setupUser();
    renderDialog([3, 1], vi.fn().mockResolvedValue(new ApiError(409, "contact 3 is in the trash")));
    await settle();
    await user.click(mergeButton());
    await settle();
    expect(screen.getByRole("alert")).toHaveTextContent("Contact 3 is in the trash");
    expect(screen.getByRole("heading", { name: "Merge 2 contacts" })).toBeInTheDocument();
  });
});
