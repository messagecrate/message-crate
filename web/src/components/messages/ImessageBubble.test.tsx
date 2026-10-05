/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { Message } from "../../lib/types";
import ImessageBubble from "./ImessageBubble";

afterEach(() => {
  cleanup();
});

function message(partial: Partial<Message> = {}): Message {
  return {
    id: 1,
    source: "imessage",
    service: "iMessage",
    guid: "g1",
    timestamp: "2026-08-11T15:04:00Z",
    is_from_me: false,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sort_order: 0,
    sender: "+1555",
    subject: null,
    text: "hi",
    attachments: [],
    tapbacks: [],
    edits: [],
    matched_earlier_version: false,
    conversation: {
      id: 1,
      chat_identifier: "x",
      conversation_type: "individual",
      is_group: false,
      group_title: null,
      participants: [{ identity: "+1555", name: "Ada", contact_id: null }],
    },
    ...partial,
  };
}

describe("ImessageBubble tapbacks", () => {
  it("renders a tapback as an emoji with a count", () => {
    render(
      <ImessageBubble
        message={message({
          tapbacks: [
            { emoji: null, is_from_me: false, kind: "loved", part_index: 0, sender: "+1555" },
          ],
        })}
      />,
    );

    expect(screen.getByText("❤️ 1")).toBeInTheDocument();
  });

  it("renders nothing tapback-related for a message with none", () => {
    render(<ImessageBubble message={message()} />);

    expect(screen.queryByText(/❤️|👍|👎|😂|‼️|❓/)).not.toBeInTheDocument();
  });
});
