import type { ReactNode } from "react";
import { deletedInSourceText, UNSENT_TEXT } from "../lib/deletionMarkText";
import { formatDay } from "../lib/formatDate";
import type { FreeTextTerm } from "../lib/freeTextTerms";
import { type MatchRange, snippet } from "../lib/messageMatch";
import { messageConversationName, messageRowText, messageSenderName } from "../lib/messageRowText";
import { useTimeZone } from "../lib/timeZone";
import { listRowDivider } from "../lib/tw";
import type { Message } from "../lib/types";
import { focusRing } from "../lib/uiStyles";
import PlainButton from "./PlainButton";

/** `text` with each range in bold. */
function boldRanges(text: string, ranges: readonly MatchRange[]): ReactNode[] {
  const out: ReactNode[] = [];
  let at = 0;
  for (const [start, end] of ranges) {
    if (start > at) out.push(text.slice(at, start));
    out.push(
      <strong key={start} className="font-semibold text-text">
        {text.slice(start, end)}
      </strong>,
    );
    at = end;
  }
  if (at < text.length) out.push(text.slice(at));
  return out;
}

/**
 * One row of the Messages list: the conversation's name and the message's
 * date in the account's Time Zone, then who sent it and its text, cut around
 * the first matching word with the matching free-text words in bold, and a
 * 📎 count when it has attachments.
 *
 * A marked message shows its mark in the words the conversation uses: one
 * Deleted in the source app keeps its text, with a muted "Deleted in
 * <source>" line under it; an Unsent one reads "Unsent" in place of its text
 * and attachments, because its sender took all of it back.
 */
export default function MessageSearchRow({
  message,
  terms,
  isSelected,
  onClick,
}: {
  message: Message;
  terms: readonly FreeTextTerm[];
  isSelected: boolean;
  onClick: () => void;
}) {
  const zone = useTimeZone();
  const unsent = message.deletion === "unsent";
  const deletedInSource = message.deletion === "deleted_in_source_app";
  const cut = snippet(unsent ? "" : messageRowText(message), terms);
  const attachmentCount = unsent ? 0 : message.attachments.length;
  const sender = messageSenderName(message);

  return (
    <PlainButton
      onPress={onClick}
      aria-current={isSelected ? "true" : undefined}
      className={`box-border flex w-full cursor-pointer flex-col gap-[0.3rem] border-none px-[0.85rem] py-[0.7rem] text-left ${focusRing} ${listRowDivider} ${
        isSelected ? "bg-hover" : "bg-transparent"
      }`}
    >
      <span className="flex min-w-0 items-baseline justify-between gap-2">
        <span className="min-w-0 flex-1 truncate text-[0.875rem] font-medium leading-[1.35] text-text">
          {messageConversationName(message.conversation)}
        </span>
        <span className="shrink-0 text-[0.75rem] text-muted">
          {formatDay(message.timestamp, zone)}
        </span>
      </span>
      <span className="flex min-w-0 items-start justify-between gap-2 text-[0.813rem] leading-[1.35]">
        <span className="line-clamp-2 min-w-0 flex-1 break-words text-muted">
          {sender ? (
            <>
              <span className="font-medium text-text">{sender}:</span>{" "}
            </>
          ) : null}
          {unsent ? UNSENT_TEXT : boldRanges(cut.text, cut.ranges)}
        </span>
        {attachmentCount > 0 ? (
          <span
            className="shrink-0 text-[0.75rem] text-muted"
            title={attachmentCount === 1 ? "1 attachment" : `${attachmentCount} attachments`}
          >
            📎 {attachmentCount}
          </span>
        ) : null}
      </span>
      {deletedInSource ? (
        <span className="text-[0.75rem] text-muted">{deletedInSourceText(message.source)}</span>
      ) : null}
    </PlainButton>
  );
}
