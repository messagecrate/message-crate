import type { SearchConversationHit } from "@/lib/search";
import type { ConversationType } from "@/lib/searchQuery";

export type SearchResultKey = `direct:${string}` | `group:${number}`;
export type SelectedSearchResultGroup = {
  key: SearchResultKey;
  conversationType: ConversationType;
  chatIdentifier: string;
  title: string;
  conversationIds: number[];
};

/** Stable key for selecting a conversation search result. */
export function searchResultKey(hit: SearchConversationHit): SearchResultKey {
  return hit.conversationType === "group"
    ? `group:${hit.conversationId}`
    : `direct:${hit.chatIdentifier}`;
}

/** Display-ordered result keys, with duplicate direct handles collapsed. */
export function orderedSearchResultKeys(
  hits: readonly SearchConversationHit[],
): SearchResultKey[] {
  const seen = new Set<SearchResultKey>();
  const out: SearchResultKey[] = [];
  for (const hit of hits) {
    const key = searchResultKey(hit);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(key);
  }
  return out;
}

/** Group selected rows into logical direct-handle or group conversations. */
export function selectedSearchResultGroups(
  hits: readonly SearchConversationHit[],
  selectedKeys: ReadonlySet<SearchResultKey>,
): SelectedSearchResultGroup[] {
  const groups = new Map<SearchResultKey, SelectedSearchResultGroup>();
  for (const hit of hits) {
    const key = searchResultKey(hit);
    if (!selectedKeys.has(key)) continue;
    const existing = groups.get(key);
    if (existing) {
      if (!existing.conversationIds.includes(hit.conversationId)) {
        existing.conversationIds.push(hit.conversationId);
      }
      continue;
    }
    groups.set(key, {
      key,
      conversationType: hit.conversationType,
      chatIdentifier: hit.chatIdentifier,
      title: hit.title,
      conversationIds: [hit.conversationId],
    });
  }
  return [...groups.values()];
}

/** Contiguous conversation-result range from an anchor to a clicked row. */
export function applySearchResultRangeSelect(
  orderedKeys: readonly SearchResultKey[],
  clickKey: SearchResultKey,
  anchorKey: SearchResultKey | null,
): Set<SearchResultKey> {
  const clickIndex = orderedKeys.indexOf(clickKey);
  if (clickIndex < 0) return new Set();
  const anchorIndex =
    anchorKey != null ? orderedKeys.indexOf(anchorKey) : -1;
  if (anchorIndex < 0) return new Set([clickKey]);
  const from = Math.min(anchorIndex, clickIndex);
  const to = Math.max(anchorIndex, clickIndex);
  return new Set(orderedKeys.slice(from, to + 1));
}

/**
 * Ordered unique contact IDs from displayed search hits.
 * Skips group conversations, unassigned handles, and duplicate contacts.
 */
export function orderedSearchContactIds(
  hits: readonly SearchConversationHit[],
): number[] {
  const seen = new Set<number>();
  const out: number[] = [];
  for (const hit of hits) {
    if (hit.conversationType !== "one-to-one") continue;
    if (hit.contactId == null) continue;
    if (seen.has(hit.contactId)) continue;
    seen.add(hit.contactId);
    out.push(hit.contactId);
  }
  return out;
}

/**
 * Contiguous range selection from an anchor contact to a clicked contact,
 * using the displayed search-result order.
 */
export function applySearchRangeSelect(
  orderedIds: readonly number[],
  clickId: number,
  anchorId: number | null,
): Set<number> {
  const clickIndex = orderedIds.indexOf(clickId);
  if (clickIndex < 0) return new Set();

  const anchorIndex =
    anchorId != null ? orderedIds.indexOf(anchorId) : -1;
  if (anchorIndex < 0) return new Set([clickId]);

  const from = Math.min(anchorIndex, clickIndex);
  const to = Math.max(anchorIndex, clickIndex);
  const next = new Set<number>();
  for (let i = from; i <= to; i++) {
    const id = orderedIds[i];
    if (id !== undefined) next.add(id);
  }
  return next;
}
