import type { ReactNode } from "react";
import { deletedInSourceText, UNSENT_TEXT } from "../lib/deletionMarkText";
import { formatDay } from "../lib/formatDate";
import { type MatchRange, matchRanges, snippet } from "../lib/messageMatch";
import { messageConversationName, messageRowText, messageSenderName } from "../lib/messageRowText";
import { useTimeZone } from "../lib/timeZone";
import { listRowDivider } from "../lib/tw";
import type { FreeTextTerm, Message } from "../lib/types";
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

/** One earlier version of one part of an edited message, as the server sends it. */
type EarlierVersion = Message["earlier_versions"][number];

/**
 * `message`'s earlier versions marked `matched`, newest first. A version's
 * time is its `edited_at`. A version without one takes the time of the
 * nearest version before it in the same part of `earlier_versions` that has
 * one, and a version with none before it is older than every dated version.
 * Of two versions with the same time, the later in `earlier_versions` is the
 * newer, because the server lists each part's versions oldest first.
 */
function matchedNewestFirst(message: Message): EarlierVersion[] {
  const partTime = new Map<number, number>();
  return message.earlier_versions
    .map((version, index) => {
      const time = version.edited_at
        ? Date.parse(version.edited_at)
        : (partTime.get(version.part_index) ?? Number.NEGATIVE_INFINITY);
      partTime.set(version.part_index, time);
      return { version, time, index };
    })
    .filter(({ version }) => version.matched)
    .sort((a, b) => (a.time === b.time ? b.index - a.index : b.time - a.time))
    .map(({ version }) => version);
}

/** Whether `version` holds `term`, the way the full-text index matches it. */
function holds(version: EarlierVersion, term: FreeTextTerm): boolean {
  return matchRanges(version.text, [term]).length > 0;
}

/**
 * The earlier versions `message`'s row quotes, each with the searched words
 * its line is cut around. The row quotes the newest matched earlier version,
 * and, for each searched word that neither that version nor the final text
 * shows, the newest matched version that holds the word. The first is cut
 * around every searched word, each other around the words it is quoted for.
 * `shownText` is the final text as the row shows it. None for a hit its
 * final text matched, which the server marks neither way
 * (`docs/architecture/search.md`).
 */
function versionsToQuote(
  message: Message,
  terms: readonly FreeTextTerm[],
  shownText: string,
): [EarlierVersion, FreeTextTerm[]][] {
  if (!message.matched_earlier_version) return [];
  const matched = matchedNewestFirst(message);
  const [first] = matched;
  if (!first) return [];
  const quoted = new Map([[first, [...terms]]]);
  for (const term of terms) {
    if (holds(first, term) || matchRanges(shownText, [term]).length > 0) continue;
    const version = matched.find((candidate) => holds(candidate, term));
    if (version) quoted.set(version, [...(quoted.get(version) ?? []), term]);
  }
  return [...quoted];
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
 *
 * A message the search found by an earlier version keeps its final text,
 * with a muted "Earlier version: …" line under it for each version it
 * quotes, cut and in bold the same way, so the row shows why it is a hit
 * (#1785). The row quotes the newest matched earlier version, and, for each
 * searched word that neither that version nor the final text shows, the
 * newest matched version that holds the word.
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
  const versionCuts = versionsToQuote(message, terms, cut.text).map(([version, words]) => {
    const text = snippet(version.text, words).text;
    return {
      key: message.earlier_versions.indexOf(version),
      text,
      ranges: matchRanges(text, terms),
    };
  });

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
      {versionCuts.map((versionCut) => (
        <span
          key={versionCut.key}
          className="line-clamp-2 min-w-0 break-words text-[0.75rem] text-muted"
        >
          Earlier version: {boldRanges(versionCut.text, versionCut.ranges)}
        </span>
      ))}
      {deletedInSource ? (
        <span className="text-[0.75rem] text-muted">{deletedInSourceText(message.source)}</span>
      ) : null}
    </PlainButton>
  );
}
