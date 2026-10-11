import { useCallback, useSyncExternalStore } from "react";

/**
 * A value kept outside React that tells its subscribers when it changes.
 *
 * It is for state that belongs to this window, not to the server: the
 * logged-in account id (`api.ts`), the desktop job holds (`desktopJob.ts`),
 * the Import Run (`importRunStore.ts`). Server data is read through TanStack
 * Query, never through a store (ADR 0002).
 */
export type ExternalStore<T> = {
  get: () => T;
  /** Replace the value, and tell every subscriber; nothing happens when it is the same value. */
  set: (next: T) => void;
  /** Call `listener` after every change. Returns the function that stops it. */
  subscribe: (listener: () => void) => () => void;
};

export function createExternalStore<T>(initial: T): ExternalStore<T> {
  let value = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set: (next) => {
      if (Object.is(next, value)) return;
      value = next;
      for (const listener of listeners) listener();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

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
