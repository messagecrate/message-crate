import { formatClockTime } from "../../lib/formatDate";
import { useTimeZone } from "../../lib/timeZone";
import MessageAttachments from "../MessageAttachments";
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
  const hasAttachments = message.attachments.length > 0;

  return (
    <ChatBubbleRow
      messageId={String(message.id)}
      mine={mine}
      isActive={isActive}
      palette="sms"
      showSender={!mine && (showSender ?? nameSender)}
      senderLabel={senderName(message)}
      timeLabel={time}
      deletion={message.deletion}
      source={message.source}
      meta={service ? <span className="uppercase tracking-[0.04em]">{service}</span> : null}
      footer={
        hasAttachments ? (
          <MessageAttachments message={message} onAttachmentClick={onAttachmentClick} />
        ) : undefined
      }
    >
      {bubbleBody(body, highlight)}
    </ChatBubbleRow>
  );
}
