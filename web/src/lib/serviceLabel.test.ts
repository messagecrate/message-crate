import { describe, expect, it } from "vitest";
import { participant } from "../test/apiShapes";
import { conversationServiceLabel } from "./serviceLabel";
import type { Conversation } from "./types";

function conversation(overrides: Partial<Conversation> = {}): Conversation {
  return {
    id: 3,
    participants: [participant({ name: "Ada Lovelace" })],
    message_count: 1,
    first_message_at: "2024-06-01T12:00:00Z",
    last_message_at: "2024-06-01T12:00:00Z",
    service: "imessage",
    is_group: false,
    shown_title: null,
    tags: [],
    ...overrides,
  };
}

describe("conversationServiceLabel", () => {
  it("calls a one-to-one conversation with no transport of its own after its participant's Text Message service", () => {
    // A conversation imported from two sources names no transport, so the
    // label falls back to the participant's stored service (#3086).
    const conv = conversation({
      service: "unknown",
      participants: [participant({ name: "Ada Lovelace", service: "text_message" })],
    });
    expect(conversationServiceLabel(conv)).toBe("Text Message");
  });

  it("keeps WhatsApp's own name", () => {
    expect(conversationServiceLabel(conversation({ service: "whatsapp" }))).toBe("whatsapp");
  });
});
