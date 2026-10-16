import { formatClockTime } from "../../lib/formatDate";
import { useTimeZone } from "../../lib/timeZone";
import { bubbleBody, type MessageBubbleProps } from "./chatBubbleShared";

/**
 * An announcement (a group renamed, a member added or removed) drawn as an
 * event of the conversation: a centered line of small, muted text with its
 * time, and no bubble, avatar or sender name (#3050). Every source draws it
 * alike. The Find term is marked in it, and the Find match it is gets the
 * active background, as a message's row does.
 */
export default function AnnouncementLine({
  message,
  highlight,
  isActive,
}: Pick<MessageBubbleProps, "message" | "highlight" | "isActive">) {
  const time = formatClockTime(message.timestamp, useTimeZone());
  const body = (message.text || "").trim();

  return (
    <div
      id={`msg-${message.id}`}
      role="note"
      className={`my-[0.4rem] px-4 py-[0.2rem] text-center text-[0.75rem] text-muted ${
        isActive ? "bg-search-active" : "bg-transparent"
      }`}
    >
      {body ? (
        <>
          <span className="whitespace-pre-wrap break-words">{bubbleBody(body, highlight)}</span> ·{" "}
        </>
      ) : null}
      <span>{time}</span>
    </div>
  );
}
