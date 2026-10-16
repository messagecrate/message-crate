import { useCallback, useSyncExternalStore } from "react";
import type { ExternalStore } from "../lib/externalStore";

/**
 * The part of `store` that `select` picks, re-rendering the caller whenever
 * the store changes. `select` must return the same value for the same state,
 * as `useSyncExternalStore` requires of a snapshot.
 */
export function useExternalStore<T, S>(
  store: Pick<ExternalStore<T>, "get" | "subscribe">,
  select: (value: T) => S,
): S {
  const read = useCallback(() => select(store.get()), [store, select]);
  return useSyncExternalStore(store.subscribe, read, read);
}
