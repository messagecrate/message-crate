import { formatClockTime } from "../../lib/formatDate";
import { useTimeZone } from "../../lib/timeZone";
import MessageAttachments from "../MessageAttachments";
import {
  bubbleBody,
  ChatBubbleRow,
  type MessageBubbleProps,
  namesSender,
  senderName,
  TapbackChips,
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
  const hasAttachments = message.attachments.length > 0;
  const hasTapbacks = message.tapbacks.length > 0;

  const footer =
    hasAttachments || hasTapbacks ? (
      <>
        <MessageAttachments message={message} onAttachmentClick={onAttachmentClick} />
        <TapbackChips message={message} className={hasAttachments ? "mt-1" : "mt-0"} />
      </>
    ) : undefined;

  return (
    <ChatBubbleRow
      message={message}
      isActive={isActive}
      palette="imessage"
      showSender={!mine && (showSender ?? nameSender)}
      senderLabel={senderName(message)}
      timeLabel={time}
      footer={footer}
    >
      {bubbleBody(body, highlight)}
    </ChatBubbleRow>
  );
}
