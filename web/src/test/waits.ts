/**
 * The wait a test gives a screen to reach a state under heavy load, for
 * `findBy*` and `waitFor`, in place of their 1000 ms default.
 *
 * The value was measured once: drawing a list's first rows takes 100 to 230 ms
 * on an idle machine and crossed the 1000 ms default at a load average near
 * 160 (#1654). 4000 ms leaves the rest of a test inside Vitest's default
 * 5000 ms budget, and the wait still ends the moment the state is reached.
 *
 * A new use needs its own measured reason, not a test that failed once: a
 * longer wait is never the fix for a slow or wrong test.
 */
export const WAIT_UNDER_LOAD: Readonly<{ timeout: number }> = { timeout: 4000 };
