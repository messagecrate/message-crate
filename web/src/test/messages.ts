import type { Message } from "../lib/types";

/** A received one-to-one Apple Messages message, "See you at noon", with `partial` on top. */
export function sampleMessage(partial: Partial<Message>): Message {
  return {
    id: 7,
    source: "imessage",
    service: "iMessage",
    guid: "g7",
    timestamp: "2026-08-11T15:04:00Z",
    is_from_me: false,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sort_order: 0,
    sender: "+15555550100",
    subject: null,
    text: "See you at noon",
    attachments: [],
    tapbacks: [],
    earlier_versions: [],
    matched_earlier_version: false,
    conversation: {
      id: 1,
      chat_identifier: "+15555550100",
      conversation_type: "individual",
      is_group: false,
      group_title: null,
      participants: [{ identity: "+15555550100", name: "Ada", contact_id: null }],
    },
    ...partial,
  };
}
