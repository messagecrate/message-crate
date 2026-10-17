import { keys } from "../lib/queryKeys";
import type { SearchField, SearchList } from "../lib/searchFields";
import { listSearchFields } from "../lib/serverApi";
import { useRouteQuery } from "./useRouteQuery";

/**
 * The words one list accepts, from the server, cached for the session. A `null`
 * list asks the server nothing and has no words.
 *
 * When the request has failed and no words were fetched before, `error` says
 * why and `fields` is empty. A caller that reads `fields` as the list's whole
 * vocabulary checks `error` first, because an empty list refuses every word.
 */
export function useSearchFields(list: SearchList | null): {
  fields: SearchField[];
  loading: boolean;
  error: Error | null;
} {
  const { data, isPending, error } = useRouteQuery(
    keys.searchFields.list(list ?? "conversations"),
    (signal) => listSearchFields(list ?? "conversations", { signal }),
    { staleTime: Number.POSITIVE_INFINITY, enabled: list !== null },
  );
  // A failed refetch keeps the words already fetched, and those still hold.
  return { fields: data ?? [], loading: isPending, error: data === undefined ? error : null };
}
