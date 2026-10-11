import { useEffect, useState } from "react";

/**
 * `value` as it stood once it stopped changing for `ms` milliseconds. A
 * caller that asks the server for each value it is given reads it through
 * this, so a value typed one key at a time is asked for once.
 */
export function useDebouncedValue<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = window.setTimeout(() => setDebounced(value), ms);
    return () => window.clearTimeout(t);
  }, [value, ms]);
  return debounced;
}
