import { describe, expect, it } from "vitest";
import { attachment, message as baseMessage, participant } from "../test/apiShapes";
import { lightboxImages } from "./lightboxImages";
import type { Message, MessageAttachment } from "./types";

function photo(name: string): MessageAttachment {
  return attachment({ original_name: name, mime_type: "image/jpeg", sha256: "aaa", path: name });
}

function message(id: number, attachments: MessageAttachment[]): Message {
  return baseMessage({
    id,
    service: "iMessage",
    sort_order: id,
    sender: "+1555",
    attachments,
    conversation: {
      id: 1,
      chat_identifier: "x",
      conversation_type: "individual",
      is_group: false,
      group_title: null,
      label: null,
      participants: [participant({ identity: "+1555", name: "Ada" })],
    },
  });
}

describe("lightboxImages", () => {
  it("opens at the image that was clicked when two messages carry the same photo", () => {
    const first = photo("first.jpg");
    const second = photo("second.jpg");
    const messages = [message(1, [first]), message(2, [second])];

    const { items, index } = lightboxImages(messages, second);

    expect(items).toEqual([first, second]);
    expect(index).toBe(1);
  });

  it("shows the clicked attachment alone when no loaded image matches", () => {
    const clicked = photo("only.jpg");

    expect(lightboxImages([], clicked)).toEqual({ items: [clicked], index: 0 });
  });
});
