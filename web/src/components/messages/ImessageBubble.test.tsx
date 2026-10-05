/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { Message } from "../../lib/types";
import { message as baseMessage, participant } from "../../test/apiShapes";
import ImessageBubble from "./ImessageBubble";

afterEach(() => {
  cleanup();
});

function message(partial: Partial<Message> = {}): Message {
  return baseMessage({
    service: "iMessage",
    sender: "+1555",
    text: "hi",
    conversation: {
      id: 1,
      chat_identifier: "x",
      conversation_type: "individual",
      is_group: false,
      group_title: null,
      label: null,
      participants: [participant({ identity: "+1555", name: "Ada" })],
    },
    ...partial,
  });
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
