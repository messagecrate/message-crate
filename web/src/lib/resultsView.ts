import { type MessageSearchSort, messageSortFromParam } from "./messageSearchSort";
import { tagListQuery } from "./messageTags";

/**
 * What the Messages screen's results list: the conversations a search
 * matches, or the messages it matches across every conversation (#313). It
 * rides in the address as `view=messages`, so it survives opening a
 * conversation and a reload, and Conversations is the address without it.
 */
export type ResultsView = "conversations" | "messages";

/** Which results the list shows: `messages`, or absent for Conversations. */
export const VIEW_PARAM = "view";
/**
 * The sort the person picked in the Messages list, spelled as `GET
 * /v1/messages` spells it. Only the Messages list keeps a sort in the address.
 */
export const MESSAGE_SORT_PARAM = "sort";
/** The message a search result opened its conversation at. */
export const AT_PARAM = "at";
/**
 * The Message Tag a conversation was opened from: the tag's name, or `none`
 * for the No Message Tag page (#1562). The tag page names the tag in its
 * path and `/messages/:id` does not, so the tag rides here, apart from `q`,
 * which holds only what the person typed.
 */
export const TAG_PARAM = "tag";

/**
 * The other list of the switch. A word only that list takes stays in the
 * search box, marked, and this list searches without it (#1561).
 */
export function otherResultsView(view: ResultsView): ResultsView {
  return view === "messages" ? "conversations" : "messages";
}

/** Which results `params` asks for. */
export function resultsView(params: URLSearchParams): ResultsView {
  return params.get(VIEW_PARAM) === "messages" ? "messages" : "conversations";
}

/** The sort the person picked in the Messages list, or null for the default. */
export function pickedMessageSort(params: URLSearchParams): MessageSearchSort | null {
  return messageSortFromParam(params.get(MESSAGE_SORT_PARAM));
}

/** The Message Tag `params` lists by on `/messages/:id`, or null for none. */
export function listedTag(params: URLSearchParams): string | null {
  return params.get(TAG_PARAM) || null;
}

/**
 * The conversations list's query on a page of the Messages screen: what was
 * typed in `q`, or `f` when a contact's link set it, within `tag`. Every page
 * builds it here from its own tag, so none can leave `q` or `f` out (#1562).
 */
export function conversationListQuery(params: URLSearchParams, tag: string | null): string {
  return tagListQuery(tag, params.get("f") || params.get("q") || "");
}

/** The message a search result opened at: a positive integer, or null. */
export function openedAt(params: URLSearchParams): number | null {
  const raw = params.get(AT_PARAM);
  if (raw === null || !/^\d+$/.test(raw)) return null;
  const n = Number(raw);
  return Number.isSafeInteger(n) && n > 0 ? n : null;
}

/**
 * The address search for a page of the Messages screen: `q`, the listed tag
 * and the Messages list's own parameters from `current`, with `overrides` on
 * top. An empty value drops the parameter. `f` is not carried: a typed search
 * replaces it.
 */
export function messagesSearch(
  current: URLSearchParams,
  overrides: Record<string, string>,
): string {
  const next = new URLSearchParams();
  for (const key of ["q", TAG_PARAM, VIEW_PARAM, MESSAGE_SORT_PARAM, AT_PARAM]) {
    const value = key in overrides ? overrides[key] : current.get(key);
    if (value) next.set(key, value);
  }
  const s = next.toString();
  return s ? `?${s}` : "";
}
