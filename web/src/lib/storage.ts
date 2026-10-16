/**
 * The one way `web/src` reads and writes `localStorage`, and `sessionStorage`
 * for a value kept only for this visit.
 *
 * A browser set to block site data throws `SecurityError` from the
 * `localStorage` getter itself, and a full or private-mode storage throws from
 * `setItem`. Every saved value here is a preference the app can do without, so
 * a blocked read gives `null` and a write that fails is dropped: the website
 * still opens, and a change applies for the visit without being saved.
 */

/**
 * How long a saved value lasts. `"kept"` is `localStorage`, kept until it is
 * removed. `"visit"` is `sessionStorage`, which the browser clears when the
 * tab closes, and which a new tab does not share.
 */
export type PrefLifetime = "kept" | "visit";

/** The browser storage for `lifetime`. Throws where the browser blocks site data. */
function storageFor(lifetime: PrefLifetime): Storage {
  return lifetime === "visit" ? window.sessionStorage : window.localStorage;
}

/** The saved value for `key`, or `null` when nothing is saved or storage is blocked. */
export function readPref(key: string, lifetime: PrefLifetime = "kept"): string | null {
  try {
    return storageFor(lifetime).getItem(key);
  } catch {
    return null;
  }
}

/** Save `value` under `key`. A blocked or full storage drops the write. */
export function writePref(key: string, value: string, lifetime: PrefLifetime = "kept"): void {
  try {
    storageFor(lifetime).setItem(key, value);
  } catch {
    // Blocked or full storage: the value lasts only for this visit.
  }
}

/**
 * `key` named with the account it belongs to. A value that records what an
 * account did, such as its recent searches, is saved under this key, so the
 * next account on the same browser never reads it and logout has nothing to
 * clear.
 */
export function accountKey(accountId: number, key: string): string {
  return `mc-account-${accountId}:${key}`;
}

/** Remove the value saved under `key`. A blocked storage leaves nothing to remove. */
export function removePref(key: string, lifetime: PrefLifetime = "kept"): void {
  try {
    storageFor(lifetime).removeItem(key);
  } catch {
    // Blocked storage: nothing was saved there.
  }
}
