import type { Message } from "../lib/types";
import { message, participant } from "./apiShapes";

/** A received one-to-one Apple Messages message, "See you at noon", with `partial` on top. */
export function sampleMessage(partial: Partial<Message>): Message {
  return message({
    id: 7,
    service: "iMessage",
    guid: "g7",
    sender: "+15555550100",
    text: "See you at noon",
    conversation: {
      id: 1,
      chat_identifier: "+15555550100",
      conversation_type: "individual",
      is_group: false,
      group_title: null,
      label: null,
      participants: [participant({ identity: "+15555550100", name: "Ada" })],
    },
    ...partial,
  });
}
