import { useEffect, useState } from "react";

/**
 * `value` as it stood once it stopped changing for `ms` milliseconds. A
 * caller that asks the server for each value it is given reads it through
 * this, so a value typed one key at a time is asked for once.
 *
 * It returns `initial` until the first `ms` have passed. `initial` defaults
 * to `value`. With `ms` of 0 or less it returns `value` in the same render,
 * and the next wait starts from it. That `value` must keep its identity
 * across renders, as a string does. A new object on every render would make
 * React stop with too many re-renders.
 */
export function useDebouncedValue<T>(value: T, ms: number, initial: T = value): T {
  const [debounced, setDebounced] = useState(initial);
  const noWait = ms <= 0;
  // Adjusted during render, not in a timer, so a change made straight after
  // cannot fall back to the value before this one.
  if (noWait && !Object.is(debounced, value)) setDebounced(value);
  useEffect(() => {
    if (noWait) return;
    const t = window.setTimeout(() => setDebounced(value), ms);
    return () => window.clearTimeout(t);
  }, [value, ms, noWait]);
  return noWait ? value : debounced;
}
