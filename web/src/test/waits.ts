/**
 * The wait a test gives a screen that must first draw, for `findBy*` and
 * `waitFor`.
 *
 * Drawing the first rows takes 100 to 230 ms on an idle machine and crossed
 * `findBy`'s 1000 ms default at a load average near 160 (#1654). 4000 ms leaves
 * the rest of the test inside Vitest's 5000 ms budget, and the wait still ends
 * the moment the element appears.
 */
export const WAIT_UNDER_LOAD = { timeout: 4000 };
