import { render } from "@testing-library/react";
import type { ComponentType } from "react";
import type { MessageBubbleProps } from "../components/messages/chatBubbleShared";
import DiscordBubble from "../components/messages/DiscordBubble";
import ImessageBubble from "../components/messages/ImessageBubble";
import InstagramBubble from "../components/messages/InstagramBubble";
import SmsBubble from "../components/messages/SmsBubble";
import WhatsAppBubble from "../components/messages/WhatsAppBubble";
import { TimeZoneContext } from "../lib/timeZone";
import type { Message } from "../lib/types";

/**
 * Every source's bubble, with the source id it is drawn for and the name the
 * product gives that source. A mark the row frame draws is tested on each, so
 * every source shows it alike (#1143).
 */
export const BUBBLES: {
  name: string;
  Bubble: ComponentType<MessageBubbleProps>;
  source: string;
  service: string;
  label: string;
}[] = [
  {
    name: "Apple Messages",
    Bubble: ImessageBubble,
    source: "imessage",
    service: "iMessage",
    label: "Apple Messages",
  },
  {
    name: "SMS Backup & Restore",
    Bubble: SmsBubble,
    source: "sms-backup-restore",
    service: "sms",
    label: "SMS Backup & Restore",
  },
  {
    name: "WhatsApp",
    Bubble: WhatsAppBubble,
    source: "whatsapp",
    service: "whatsapp",
    label: "WhatsApp",
  },
  {
    name: "Discord",
    Bubble: DiscordBubble,
    source: "discord",
    service: "discord",
    label: "Discord",
  },
  {
    name: "Instagram",
    Bubble: InstagramBubble,
    source: "instagram",
    service: "instagram",
    label: "Instagram",
  },
];

/** A received one-to-one Apple Messages message, "See you at noon", with `partial` on top. */
export function bubbleMessage(partial: Partial<Message>): Message {
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

/** The bubble in UTC, so its time reads the same on every machine. */
export function renderBubbleInUtc(Bubble: ComponentType<MessageBubbleProps>, m: Message) {
  return render(
    <TimeZoneContext.Provider value="UTC">
      <Bubble message={m} />
    </TimeZoneContext.Provider>,
  );
}
