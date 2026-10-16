/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { TimeZoneContext } from "../lib/timeZone";
import type { Message } from "../lib/types";
import { message, participant } from "../test/apiShapes";
import { BUBBLES } from "../test/bubbles";
import MessageBubble from "./MessageBubble";

afterEach(() => {
  cleanup();
});

/** Ada renaming a group she shares with Bo, from `source`, at 15:04 UTC. */
function renaming(source: string, service: string): Message {
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
  });
}

describe("MessageBubble announcements", () => {
  for (const { source, service, label } of BUBBLES) {
    it(`draws a ${label} announcement as a line with its time, with no sender or bubble`, () => {
      render(
        <TimeZoneContext.Provider value="UTC">
          <MessageBubble message={renaming(source, service)} highlight="trip" />
        </TimeZoneContext.Provider>,
      );

      const line = screen.getByRole("note");
      expect(line).toHaveTextContent("Ada named the conversation “Trip” · 3:04 PM");
      expect(line.querySelector("mark")).toHaveTextContent("Trip");
      expect(screen.queryByText("Ada")).not.toBeInTheDocument();
      expect(screen.queryByText(service, { exact: false })).not.toBeInTheDocument();
    });
  }
});
