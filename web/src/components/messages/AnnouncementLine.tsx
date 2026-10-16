import { formatClockTime } from "../../lib/formatDate";
import { useTimeZone } from "../../lib/timeZone";
import MessageAttachments from "../MessageAttachments";
import {
  bubbleBody,
  DeletionNote,
  type MessageBubbleProps,
  MessageRowFrame,
  TapbackChips,
} from "./chatBubbleShared";

/**
 * An announcement (a group renamed, a member added or removed) drawn as an
 * event of the conversation: a centered line of small, muted text with its
 * time, and no bubble, avatar or sender name (#3050). Every source draws it
 * alike. The Find term is marked in it, and when it is the active Find match,
 * it gets the active background, as a message's row does. What else it
 * carries is kept: the "Deleted in" note after its time, and under the line
 * its attachments, such as a new group photo, and its reactions.
 */
export default function AnnouncementLine({
  message,
  highlight,
  isActive,
  onAttachmentClick,
}: Omit<MessageBubbleProps, "showSender">) {
  const time = formatClockTime(message.timestamp, useTimeZone());
  const body = (message.text || "").trim();

  return (
    <MessageRowFrame
      messageId={message.id}
      isActive={isActive}
      className="my-[0.4rem] flex flex-col items-center px-4 py-[0.2rem]"
    >
      <div role="note" className="text-center text-[0.75rem] text-muted">
        {body ? (
          <>
            <span className="whitespace-pre-wrap break-words">{bubbleBody(body, highlight)}</span> ·{" "}
          </>
        ) : null}
        <span>{time}</span> <DeletionNote deletion={message.deletion} source={message.source} />
      </div>
      <MessageAttachments message={message} onAttachmentClick={onAttachmentClick} />
      <TapbackChips message={message} className="mt-1 justify-center" />
    </MessageRowFrame>
  );
}
