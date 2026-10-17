import type { MessagesListParams } from "./serverApi";
import type { SortOrder } from "./sortOrder";

/** What the Messages list can be sorted by. */
export type MessageSearchSortKey = "relevance" | "date";

/** One choice in the Messages list's sort menu. Relevance has no order: best match first. */
export type MessageSearchSort = { sort: MessageSearchSortKey; order: SortOrder };

/** A `sort` value `GET /v1/messages` takes. */
export type MessageSortParam = NonNullable<MessagesListParams["sort"]>;

/**
 * The `sort` value for each choice, read both ways: Relevance has one order,
 * and Date one value per order.
 */
const SORT_PARAMS = {
  relevance: "relevance",
  asc: "date",
  desc: "-date",
} as const satisfies Readonly<Record<"relevance" | SortOrder, MessageSortParam>>;

/** The `sort` parameter `GET /v1/messages` takes for `s`. */
export function messageSortParam(s: MessageSearchSort): MessageSortParam {
  return SORT_PARAMS[s.sort === "relevance" ? "relevance" : s.order];
}

/**
 * The choice a `sort` value stands for, or null for a value it is not. A
 * value of several keys, such as `relevance,-date`, stands for its first,
 * which decides the order.
 */
export function messageSortFromParam(param: string | null): MessageSearchSort | null {
  const first = param?.split(",")[0]?.trim() ?? null;
  if (first === SORT_PARAMS.relevance) return { sort: "relevance", order: "desc" };
  if (first === SORT_PARAMS.asc) return { sort: "date", order: "asc" };
  if (first === SORT_PARAMS.desc) return { sort: "date", order: "desc" };
  return null;
}

/**
 * What the person's pick in the sort menu keeps: a Date order as its `sort`
 * value, or nothing for Relevance.
 *
 * Relevance is never kept as a pick. The menu offers it only for a search the
 * server already ranks when no `sort` is sent, so picking it means "the
 * server's order". A kept `relevance` would follow the person to their next
 * search, and the server refuses it for a search with no free-text word.
 */
export function pickedSortParam(s: MessageSearchSort): (typeof SORT_PARAMS)[SortOrder] | null {
  return s.sort === "relevance" ? null : SORT_PARAMS[s.order];
}
