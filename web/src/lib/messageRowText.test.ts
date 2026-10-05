import { describe, expect, it } from "vitest";
import { attachment, message as baseMessage, participant } from "../test/apiShapes";
import { messageConversationName, messageRowText, messageSenderName } from "./messageRowText";
import type { Message, MessageConversation } from "./types";

function conversation(over: Partial<MessageConversation> = {}): MessageConversation {
  return {
    id: 1,
    chat_identifier: "+15555550100",
    conversation_type: "individual",
    is_group: false,
    group_title: null,
    label: null,
    participants: [{ name: "Alice", identity: "+15555550100", contact_id: 4, service: "imessage" }],
    ...over,
  };
}

function message(over: Partial<Message> = {}): Message {
  return baseMessage({
    id: 10,
    guid: "g10",
    timestamp: "2024-01-01T10:00:00Z",
    sender: "+15555550100",
    text: "hello",
    conversation: conversation(),
    ...over,
  });
}

describe("messageConversationName", () => {
  it("names a one-to-one conversation after the other person", () => {
    expect(messageConversationName(conversation())).toBe("Alice");
  });

  it("names a conversation with yourself by the title the server gives it", () => {
    // The server titles it with the account's display name, or the address;
    // it has no participants to fall back on.
    expect(
      messageConversationName(
        conversation({ chat_identifier: "+15555550199", participants: [], label: "Sam Holder" }),
      ),
    ).toBe("Sam Holder");
  });

  it("names a group conversation by its title, else by everyone in it", () => {
    const people = [
      participant({ name: "Alice", identity: "+1" }),
      participant({ name: "Bob", identity: "+2" }),
    ];
    expect(
      messageConversationName(
        conversation({
          conversation_type: "group",
          is_group: true,
          label: " Family ",
          participants: people,
        }),
      ),
    ).toBe("Family");
    expect(
      messageConversationName(
        conversation({ conversation_type: "group", is_group: true, participants: people }),
      ),
    ).toBe("Alice, Bob");
  });

  it("takes whether a conversation is a group from the server, never from its type", () => {
    // The server decides `is_group` for both lists, so a type the browser
    // does not know still names the conversation as the conversation list does.
    const people = [
      participant({ name: "Alice", identity: "+1" }),
      participant({ name: "Bob", identity: "+2" }),
    ];
    expect(
      messageConversationName(
        conversation({ conversation_type: "chat", is_group: true, participants: people }),
      ),
    ).toBe("Alice, Bob");
  });
});

describe("messageSenderName", () => {
  it('is "You" for a message the account sent', () => {
    expect(messageSenderName(message({ is_from_me: true, sender: null }))).toBe("You");
  });

  it("is the participant's name for their identity, else the identity", () => {
    expect(messageSenderName(message())).toBe("Alice");
    expect(messageSenderName(message({ sender: "+15555550198" }))).toBe("+15555550198");
  });

  it("is null for a received message that names no sender", () => {
    expect(messageSenderName(message({ sender: null }))).toBeNull();
  });
});

describe("messageRowText", () => {
  it("is the message's text, or its attachments' names when it has none", () => {
    expect(messageRowText(message())).toBe("hello");
    expect(
      messageRowText(
        message({
          text: null,
          attachments: [
            attachment({ original_name: "IMG_0001.jpg" }),
            attachment({ original_name: "notes.pdf" }),
          ],
        }),
      ),
    ).toBe("IMG_0001.jpg, notes.pdf");
  });
});
