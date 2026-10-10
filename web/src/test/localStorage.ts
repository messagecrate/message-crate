import { vi } from "vitest";

/**
 * Installs an empty in-memory `localStorage`, as both the global and
 * `window.localStorage`, and returns it. The node environment has no window,
 * and lib/storage.ts reads window.localStorage. Undo it with
 * `vi.unstubAllGlobals()`.
 */
export function installMemoryStorage(): Storage {
  const mem = new Map<string, string>();
  const store: Storage = {
    getItem: (k) => mem.get(k) ?? null,
    setItem: (k, v) => {
      mem.set(k, String(v));
    },
    removeItem: (k) => {
      mem.delete(k);
    },
    clear: () => mem.clear(),
    key: () => null,
    length: 0,
  };
  vi.stubGlobal("localStorage", store);
  vi.stubGlobal("window", { localStorage: store });
  return store;
}
