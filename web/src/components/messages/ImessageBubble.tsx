import { formatClockTime } from "../../lib/formatDate";
import { useTimeZone } from "../../lib/timeZone";
import {
  bubbleBody,
  ChatBubbleRow,
  type MessageBubbleProps,
  namesSender,
  senderName,
} from "./chatBubbleShared";

export default function ImessageBubble({
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

  return (
    <ChatBubbleRow
      message={message}
      isActive={isActive}
      palette="imessage"
      showSender={!mine && (showSender ?? nameSender)}
      senderLabel={senderName(message)}
      timeLabel={time}
      onAttachmentClick={onAttachmentClick}
    >
      {bubbleBody(body, highlight)}
    </ChatBubbleRow>
  );
}
