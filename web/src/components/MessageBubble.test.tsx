/** @vitest-environment jsdom */

import { cleanup, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { Message } from "../lib/types";
import { attachment, message, participant } from "../test/apiShapes";
import { BUBBLES, renderBubbleInUtc } from "../test/bubbles";
import MessageBubble from "./MessageBubble";

afterEach(() => {
  cleanup();
});

/** Ada renaming a group she shares with Bo, from `source`, at 15:04 UTC. */
function renaming(source: string, service: string, fields: Partial<Message> = {}): Message {
  return message({
    source,
    service,
    sender: "+1555",
    is_announcement: true,
    text: "Ada named the conversation “Trip”",
    conversation: {
      id: 1,
      chat_identifier: "x",
      conversation_type: "group",
      is_group: true,
      group_title: "Trip",
      shown_title: "Trip",
      participants: [
        participant({ identity: "+1555", name: "Ada" }),
        participant({ identity: "+1666", name: "Bo" }),
      ],
    },
    ...fields,
  });
}

/** `MessageBubble` with the Find term "trip", as a conversation draws it while Find is open. */
function FindingTrip({ message }: { message: Message }) {
  return <MessageBubble message={message} highlight="trip" />;
}

describe("MessageBubble announcements", () => {
  for (const { source, service, label } of BUBBLES) {
    it(`draws a ${label} announcement as a line with its time and no sender`, () => {
      renderBubbleInUtc(FindingTrip, renaming(source, service));

      const line = screen.getByRole("note");
      expect(line).toHaveTextContent("Ada named the conversation “Trip” · 3:04 PM");
      expect(line.querySelector("mark")).toHaveTextContent("Trip");
      expect(screen.queryByText("Ada")).not.toBeInTheDocument();
      expect(screen.queryByText(service, { exact: false })).not.toBeInTheDocument();
    });
  }

  it("keeps an announcement's attachments, reactions and deletion mark", () => {
    renderBubbleInUtc(
      FindingTrip,
      renaming("imessage", "iMessage", {
        text: "Ada changed the group photo.",
        deletion: "deleted_in_source_app",
        attachments: [attachment({ original_name: "group.jpg", missing_reason: "file_missing" })],
        tapbacks: [
          {
            emoji: null,
            is_from_me: false,
            kind: "loved",
            part_index: 0,
            sender: "+1666",
            sender_display_name: null,
          },
        ],
      }),
    );

    expect(screen.getByRole("note")).toHaveTextContent(
      "Ada changed the group photo. · 3:04 PM · Deleted in Apple Messages",
    );
    expect(screen.getByText("group.jpg (missing — file not found)")).toBeInTheDocument();
    expect(screen.getByText("❤️ 1")).toHaveAttribute("title", "Bo");
  });
});
