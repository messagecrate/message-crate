import { hasFieldToken, type MarkedWord, markedWords, type SearchList } from "../lib/searchFields";
import { dropTokens } from "../lib/searchQuery";
import { useSearchFields } from "./useSearchFields";

/**
 * `query` as `list` searches it: without the words only `otherList` takes
 * (#1561). `ready` is `useMarkedWords`'s. The results lists and Export both
 * read their query through this, so they leave out the same words.
 */
export function useListQuery(
  query: string,
  list: SearchList,
  otherList: SearchList,
): { listQuery: string; marked: MarkedWord[]; ready: boolean } {
  const { marked, ready } = useMarkedWords(query, list, otherList);
  return { listQuery: dropTokens(query, marked), marked, ready };
}

/**
 * The words of `query` that `list` does not take and `otherList` does, which
 * the search box marks and the list leaves out of its search (#1561).
 *
 * `ready` is false while the words of either list are still being fetched
 * and the query has a `word:` token, so a list can wait rather than send a
 * word the server would refuse. When either fetch fails, nothing is marked:
 * the server answers the query as typed. A `null` list or other list marks
 * nothing and asks the server nothing.
 */
export function useMarkedWords(
  query: string,
  list: SearchList | null,
  otherList: SearchList | null,
): { marked: MarkedWord[]; ready: boolean } {
  const both = list !== null && otherList !== null;
  const here = useSearchFields(both ? list : null);
  const there = useSearchFields(both ? otherList : null);
  if (!both || !hasFieldToken(query)) return { marked: [], ready: true };
  if (here.error || there.error) return { marked: [], ready: true };
  if (here.loading || there.loading) return { marked: [], ready: false };
  return {
    marked: markedWords(query, here.fields, { list: otherList, fields: there.fields }),
    ready: true,
  };
}
