import { hasFieldToken } from "./searchFields";
import { useDebouncedValue } from "./useDebouncedValue";

/** How long a list waits after the last keystroke before it searches. */
const QUERY_DEBOUNCE_MS = 300;

/**
 * `query` as a list searches it: after the person stops typing for
 * {@link QUERY_DEBOUNCE_MS}, or at once when it names a word (`from:`), so the
 * list does not flash empty while a word is typed. The Conversations and
 * Messages lists both read their query through this.
 */
export function useDebouncedQuery(query: string): string {
  return useDebouncedValue(query, hasFieldToken(query) ? 0 : QUERY_DEBOUNCE_MS);
}
