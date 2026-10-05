/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { Message } from "../lib/types";
import { inTimeZone } from "../test/timeZone";
import MessageSearchRow from "./MessageSearchRow";

afterEach(cleanup);

/** The account's zone every row here renders in. */
const ZONE = "America/New_York";

function message(over: Partial<Message> = {}): Message {
  return {
    id: 10,
    source: "imessage",
    guid: "g10",
    // 03:30 UTC on 2 January is still 1 January in New York.
    timestamp: "2024-01-02T03:30:00Z",
    sort_order: 0,
    is_from_me: false,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sender: "+15555550100",
    text: "Here is the photo from the dentist",
    conversation: {
      id: 1,
      chat_identifier: "chat1",
      conversation_type: "group",
      is_group: true,
      group_title: "Family",
      label: "Family",
      participants: [
        { name: "Alice", identity: "+15555550100" },
        { name: "Bob", identity: "+15555550200" },
      ],
    },
    attachments: [],
    tapbacks: [],
    ...over,
  };
}

function renderRow(m: Message, terms = [{ text: "photo", prefix: false }]) {
  return render(
    inTimeZone(
      ZONE,
      <MessageSearchRow message={m} terms={terms} isSelected={false} onClick={() => {}} />,
    ),
  );
}

describe("MessageSearchRow", () => {
  it("shows the conversation, the day in the account's zone, the sender, and the matching word in bold", () => {
    const m = message();
    renderRow(m);
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("Family");
    expect(row).toHaveTextContent(
      new Date(m.timestamp).toLocaleDateString([], {
        timeZone: ZONE,
        year: "numeric",
        month: "short",
        day: "numeric",
      }),
    );
    expect(row).toHaveTextContent("Alice: Here is the photo from the dentist");
    const bold = row.querySelectorAll("strong");
    expect([...bold].map((b) => b.textContent)).toEqual(["photo"]);
  });

  it('says "You" for a sent message, and counts its attachments', () => {
    renderRow(
      message({
        is_from_me: true,
        sender: null,
        attachments: [{ original_name: "a.jpg" }, { original_name: "b.jpg" }],
      }),
    );
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("You:");
    expect(screen.getByTitle("2 attachments")).toHaveTextContent("📎 2");
  });

  it("shows no sender for a received message that names none", () => {
    renderRow(message({ sender: null }));
    const row = screen.getByRole("button");
    expect(row).not.toHaveTextContent("Unknown");
    expect(row).not.toHaveTextContent(":");
  });

  it("shows the attachments' names for a message with no text, with a matching name in bold", () => {
    renderRow(message({ text: null, attachments: [{ original_name: "photo 1.jpg" }] }));
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("Alice: photo 1.jpg");
    expect(row.querySelector("strong")?.textContent).toBe("photo");
  });

  it("keeps the text of a message Deleted in the source app and notes the source by its name", () => {
    renderRow(message({ deletion: "deleted_in_source_app" }));
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("Alice: Here is the photo from the dentist");
    const note = screen.getByText("Deleted in Apple Messages");
    expect(note.closest(".text-muted")).not.toBeNull();
    expect(row).not.toHaveTextContent("Unsent");
  });

  it('shows "Unsent" in place of the text of an Unsent message, in the muted colour', () => {
    renderRow(
      message({
        deletion: "unsent",
        text: null,
        attachments: [{ original_name: "photo 1.jpg" }],
      }),
    );
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("Alice: Unsent");
    expect(row).not.toHaveTextContent("photo 1.jpg");
    expect(row).not.toHaveTextContent("📎");
    expect(row).not.toHaveTextContent("Deleted in");
    expect(screen.getByText("Unsent").closest(".text-muted")).not.toBeNull();
  });

  it("carries no mark for an unmarked message", () => {
    renderRow(message());
    const row = screen.getByRole("button");
    expect(row).not.toHaveTextContent("Deleted in");
    expect(row).not.toHaveTextContent("Unsent");
  });
});
