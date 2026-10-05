import type { Message } from "./types";

/**
 * Where the earlier versions a search found `message` by sit in its list,
 * oldest first: the versions marked `matched` on a hit found only by an
 * earlier version. Empty for any other message, a hit its final text matched
 * included (`docs/architecture/search.md`).
 */
export function matchedVersionIndexes(message: Message): number[] {
  if (!message.matched_earlier_version) return [];
  return message.earlier_versions.flatMap((version, index) => (version.matched ? [index] : []));
}

/**
 * `message` as a search found it only by the earlier versions at `indexes`:
 * marked so, with those versions marked `matched` and the rest not. The
 * conversation reads its messages without a search, so this is how the one
 * a search result or a Find match opened at is drawn opened, with the
 * versions it was found by highlighted (#1143). `message` as it is when none
 * of `indexes` is one of its versions.
 */
export function withMatchedVersions(message: Message, indexes: readonly number[]): Message {
  const found = new Set(indexes);
  if (!message.earlier_versions.some((_, index) => found.has(index))) return message;
  return {
    ...message,
    matched_earlier_version: true,
    earlier_versions: message.earlier_versions.map((version, index) => ({
      ...version,
      matched: found.has(index),
    })),
  };
}
