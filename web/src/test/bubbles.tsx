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
 * product gives that source (`label`). A mark the row frame draws is tested on
 * each, so every source shows it alike (#1143). `drawsReactions` says whether
 * the bubble draws a message's reactions, so a test about reactions runs only
 * where it can fail.
 */
export const BUBBLES: {
  Bubble: ComponentType<MessageBubbleProps>;
  source: string;
  service: string;
  label: string;
  drawsReactions: boolean;
}[] = [
  {
    Bubble: ImessageBubble,
    source: "imessage",
    service: "iMessage",
    label: "Apple Messages",
    drawsReactions: true,
  },
  {
    Bubble: SmsBubble,
    source: "sms-backup-restore",
    service: "sms",
    label: "SMS Backup & Restore",
    drawsReactions: false,
  },
  {
    Bubble: WhatsAppBubble,
    source: "whatsapp",
    service: "whatsapp",
    label: "WhatsApp",
    drawsReactions: false,
  },
  {
    Bubble: DiscordBubble,
    source: "discord",
    service: "discord",
    label: "Discord",
    drawsReactions: true,
  },
  {
    Bubble: InstagramBubble,
    source: "instagram",
    service: "instagram",
    label: "Instagram",
    drawsReactions: false,
  },
];

/**
 * The bubble in UTC, so its time reads the same on every machine.
 * `rerenderWith` draws the same bubble with another message, as a thread does
 * when a message's answer changes, keeping its state.
 */
export function renderBubbleInUtc(Bubble: ComponentType<MessageBubbleProps>, m: Message) {
  const inUtc = (message: Message) => (
    <TimeZoneContext.Provider value="UTC">
      <Bubble message={message} />
    </TimeZoneContext.Provider>
  );
  const result = render(inUtc(m));
  return { ...result, rerenderWith: (message: Message) => result.rerender(inUtc(message)) };
}
