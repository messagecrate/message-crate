import { formatClockTime } from "../../lib/formatDate";
import { useTimeZone } from "../../lib/timeZone";
import {
  bubbleBody,
  ChatBubbleRow,
  type MessageBubbleProps,
  namesSender,
  senderName,
} from "./chatBubbleShared";

/** SMS / MMS / RCS / Android Messages — green sent bubbles. */
export default function SmsBubble({
  message,
  highlight,
  isActive,
  showSender,
  onAttachmentClick,
}: MessageBubbleProps) {
  const time = formatClockTime(message.timestamp, useTimeZone());
  const mine = message.is_from_me;
  const nameSender = namesSender(message);
  const body = (message.text || "").trim();
  const service = message.service?.trim() || message.source?.trim();

  return (
    <ChatBubbleRow
      message={message}
      isActive={isActive}
      palette="sms"
      showSender={!mine && (showSender ?? nameSender)}
      senderLabel={senderName(message)}
      timeLabel={time}
      meta={service ? <span className="uppercase tracking-[0.04em]">{service}</span> : null}
      onAttachmentClick={onAttachmentClick}
    >
      {bubbleBody(body, highlight)}
    </ChatBubbleRow>
  );
}
