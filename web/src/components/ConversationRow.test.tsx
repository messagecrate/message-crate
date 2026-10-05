/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { formatDay } from "../lib/formatDate";
import { browserTimeZone } from "../lib/timeZone";
import type { Conversation } from "../lib/types";
import { participant } from "../test/apiShapes";
import ConversationRow from "./ConversationRow";

function conversation(overrides: Partial<Conversation> = {}): Conversation {
  return {
    id: 3,
    participants: [participant({ name: "Ada Lovelace" })],
    message_count: 1,
    first_message_at: "2024-06-01T12:00:00Z",
    last_message_at: "2024-06-01T12:00:00Z",
    service: "imessage",
    is_group: false,
    label: null,
    tags: [],
    ...overrides,
  };
}

describe("ConversationRow", () => {
  afterEach(() => {
    cleanup();
  });

  it("shows the day of the last message", () => {
    render(<ConversationRow conversation={conversation()} isSelected={false} onClick={() => {}} />);
    expect(screen.getByText(formatDay("2024-06-01T12:00:00Z", browserTimeZone()))).toBeTruthy();
  });

  it("shows no date for a conversation whose messages are all duplicates", () => {
    // The server sends `first_message_at` and `last_message_at` as null when
    // no message is left once duplicates are set aside (issues #1206, #1484).
    const undated = conversation({
      message_count: 0,
      first_message_at: null,
      last_message_at: null,
    });
    render(<ConversationRow conversation={undated} isSelected={false} onClick={() => {}} />);
    const row = screen.getByRole("button");
    expect(row.textContent).toBe("Ada LovelaceText Message");
  });

  it("names a group with nobody in it as the Messages list does", () => {
    render(
      <ConversationRow
        conversation={conversation({ is_group: true, participants: [] })}
        isSelected={false}
        onClick={() => {}}
      />,
    );
    expect(screen.getByRole("button").textContent).toContain("(unknown)");
  });
});
